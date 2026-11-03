//! Agent-facing defaults and one-pass result ownership, outside the shared dispatcher.
use super::completion::{self, Caller};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io,
    time::Duration,
};
use tokio::time::Instant;

#[derive(Default)]
pub(crate) struct Delivery {
    // A page owns only its own source/derived chain, never another investigation.
    pages: HashMap<String, PageDelivery>,
    consumed_jobs: HashMap<String, Instant>,
}

struct PageDelivery {
    created: Instant,
    captures: HashSet<String>,
    job: Option<String>,
    log: Option<String>,
}

pub(crate) async fn call(
    client: &mut (impl Caller + Send),
    delivery: &mut Delivery,
    method: &str,
    mut args: Value,
) -> io::Result<Value> {
    delivery
        .pages
        .retain(|_, page| page.created.elapsed() < Duration::from_secs(300));
    delivery
        .consumed_jobs
        .retain(|_, created| created.elapsed() < Duration::from_secs(300));
    let retain = args
        .as_object_mut()
        .unwrap()
        .remove("retain_capture")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let schema = super::catalog::TOOLS
        .iter()
        .find(|t| t.name == method)
        .expect("validated tool")
        .input_schema
        .clone();
    if schema["properties"].get("wait_ms").is_some() && args.get("wait_ms").is_none() {
        args["wait_ms"] = json!(5000);
    }
    if schema["properties"].get("read").is_some() && args.get("read").is_none() {
        args["read"] = json!({});
    }
    if schema["properties"].get("limit").is_some() && args.get("limit").is_none() {
        args["limit"] = json!(5000);
    }
    if let Some(read) = args.get_mut("read")
        && read.get("limit").is_none()
    {
        read["limit"] = json!(5000);
    }
    let read = args.get("read").cloned();
    let previous_capture = args["capture_id"].as_str().map(str::to_owned);
    let job = args["job_id"].as_str().map(str::to_owned);
    let closing_log = args["log_ref"].as_str().map(str::to_owned);
    // A completed one-pass job reports its outcome without attempting to resurrect consumed rows.
    if job
        .as_ref()
        .is_some_and(|id| delivery.consumed_jobs.contains_key(id))
    {
        args.as_object_mut().unwrap().remove("read");
    }
    let mut value = completion::call(client, method, args).await?;
    if value.get("error").is_some() {
        return Ok(value);
    }
    if method == "winproc_close_log" {
        delivery
            .pages
            .retain(|_, page| page.log.as_ref() != closing_log.as_ref());
    }
    if method == "winproc_release_capture"
        && let Some(target) = previous_capture.as_ref()
    {
        let keys: Vec<_> = delivery
            .pages
            .iter()
            .filter(|(_, page)| page.captures.contains(target))
            .map(|(id, _)| id.clone())
            .collect();
        for key in keys {
            if let Some(PageDelivery {
                captures: owned,
                job,
                ..
            }) = delivery.pages.remove(&key)
            {
                for id in owned.into_iter().filter(|id| id != target) {
                    let result = client
                        .call("winproc_release_capture", json!({"capture_id":id}))
                        .await?;
                    if result.get("error").is_some() {
                        return Err(io::Error::other("Result cleanup failed"));
                    }
                }
                if let Some(job) = job {
                    delivery.consumed_jobs.insert(job, Instant::now());
                }
            }
        }
    }
    let job = value["data"]["job_id"].as_str().map(str::to_owned).or(job);
    if job
        .as_ref()
        .is_some_and(|id| delivery.consumed_jobs.contains_key(id))
    {
        value["data"]["capture_retained"] = json!(false);
        value["data"]["result_consumed"] = json!(true);
        value["data"]["next_action"] = Value::Null;
        return Ok(value);
    }
    let source = value["data"]["result"]["capture_id"]
        .as_str()
        .map(str::to_owned);
    let nested = value["data"].get("result_page").is_some();
    let page = if nested {
        &mut value["data"]["result_page"]
    } else {
        &mut value
    };
    if page.get("error").is_none() && page["data"]["rows"].is_array() {
        let id = page["data"]["capture_id"]
            .as_str()
            .expect("page capture")
            .to_owned();
        let prior = previous_capture
            .as_ref()
            .and_then(|id| delivery.pages.remove(id));
        let PageDelivery {
            created,
            captures: mut owned,
            job: inherited_job,
            log: inherited_log,
        } = prior.unwrap_or_else(|| PageDelivery {
            created: Instant::now(),
            captures: HashSet::new(),
            job: None,
            log: None,
        });
        let log = page["log_ref"]
            .as_str()
            .map(str::to_owned)
            .or(inherited_log);
        let job = job.or(inherited_job);
        owned.insert(id.clone());
        if let Some(source) = source {
            owned.insert(source);
        }
        if let Some(previous) = previous_capture {
            owned.insert(previous);
        }
        let cursor = page["data"]["next_cursor"].as_str().map(str::to_owned);
        let action = cursor.as_ref().map(|cursor| {
            let mut arguments = json!({"capture_id":id,"cursor":cursor});
            if retain {
                arguments["retain_capture"] = json!(true);
            }
            json!({"tool":"winproc_read_capture","arguments":arguments})
        });
        if retain {
            let mut ids: Vec<_> = owned.into_iter().collect();
            ids.sort();
            page["data"]["retained_capture_ids"] = json!(ids);
            page["data"]["capture_retained"] = json!(true);
        } else if cursor.is_some() {
            delivery.pages.insert(
                id.clone(),
                PageDelivery {
                    created,
                    captures: owned,
                    job,
                    log,
                },
            );
            if delivery.pages.len() > 8 {
                // Backend invalidation can precede a later bridge request. Bound stale
                // bookkeeping without releasing another investigation's actual data.
                let oldest = delivery
                    .pages
                    .iter()
                    .filter(|(key, _)| **key != id)
                    .min_by_key(|(_, page)| page.created)
                    .map(|(key, _)| key.clone());
                if let Some(oldest) = oldest {
                    delivery.pages.remove(&oldest);
                }
            }
            page["data"]["capture_retained"] = json!(true);
        } else {
            // Release only after copying the complete final page. Any transport loss aborts
            // the bridge connection; the application then releases all connection-owned data.
            for id in owned {
                let released = client
                    .call("winproc_release_capture", json!({"capture_id":id}))
                    .await?;
                if released.get("error").is_some() {
                    return Err(io::Error::other("Result cleanup failed"));
                }
            }
            page["data"]["capture_retained"] = json!(false);
            if let Some(job) = job {
                delivery.consumed_jobs.insert(job, Instant::now());
            }
        }
        page["data"]["next_action"] = action.unwrap_or(Value::Null);
        if nested {
            value["data"]["next_action"] =
                value["data"]["result_page"]["data"]["next_action"].clone();
            value["data"]["capture_retained"] =
                value["data"]["result_page"]["data"]["capture_retained"].clone();
        }
    } else if nested {
        // A page-selection error leaves the source available for a corrected read.
        value["data"]["capture_retained"] = json!(true);
        value["data"]["retained_capture_ids"] = json!(source.into_iter().collect::<Vec<_>>());
        value["data"]["next_action"] = Value::Null;
    } else if matches!(value["data"]["state"].as_str(), Some("running" | "pending")) {
        let mut arguments;
        let tool;
        if let Some(id) = value["data"]["job_id"].as_str() {
            tool = "winproc_get_job";
            arguments = json!({"job_id":id});
            if let Some(read) = read {
                arguments["read"] = read;
            }
            if retain {
                arguments["retain_capture"] = json!(true);
            }
        } else if let Some(id) = value["data"]["operation_id"].as_str() {
            tool = "winproc_get_operation";
            arguments = json!({"operation_id":id});
        } else {
            return Ok(value);
        }
        value["data"]["next_action"] = json!({"tool":tool,"arguments":arguments});
    } else if value["data"]["state"].is_string() {
        // In particular, local confirmation is not a reason to auto-poll or replay.
        value["data"]["next_action"] = Value::Null;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake {
        replies: VecDeque<Value>,
        calls: Vec<(String, Value)>,
    }
    impl Caller for Fake {
        async fn call(&mut self, method: &str, args: Value) -> io::Result<Value> {
            self.calls.push((method.into(), args));
            Ok(self.replies.pop_front().expect("unexpected backend call"))
        }
    }
    fn fake(replies: Vec<Value>) -> Fake {
        Fake {
            replies: replies.into(),
            calls: vec![],
        }
    }
    fn page(id: &str, cursor: Value) -> Value {
        json!({"status":"partial","warnings":["coverage"],"data":{"capture_id":id,"rows":[{"path":"x"}],"next_cursor":cursor}})
    }
    #[tokio::test]
    async fn default_finishes_reads_and_releases_only_its_source_and_projection() {
        let mut c = fake(vec![
            json!({"data":{"job_id":"j","state":"running"}}),
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
            page("derived", Value::Null),
            json!({"data":{"released":true}}),
            json!({"data":{"released":true}}),
        ]);
        let mut delivery = Delivery::default();
        let r = call(
            &mut c,
            &mut delivery,
            "winproc_start_capture",
            json!({"kind":"file_users","read":{"projection":["path"],"distinct_paths":true}}),
        )
        .await
        .unwrap();
        assert_eq!(c.calls.len(), 5);
        assert_eq!(c.calls[0].1, json!({"kind":"file_users"}));
        assert_eq!(c.calls[2].1["limit"], 5000);
        let released: HashSet<_> = c.calls[3..]
            .iter()
            .map(|(_, a)| a["capture_id"].as_str().unwrap())
            .collect();
        assert_eq!(released, HashSet::from(["source", "derived"]));
        assert_eq!(r["data"]["result_page"]["status"], "partial");
        assert_eq!(r["data"]["capture_retained"], false);
        assert!(r["data"]["next_action"].is_null());
        c.replies.push_back(
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
        );
        let replay = call(
            &mut c,
            &mut delivery,
            "winproc_get_job",
            json!({"job_id":"j"}),
        )
        .await
        .unwrap();
        assert_eq!(replay["data"]["result_consumed"], true);
        assert_eq!(c.calls.len(), 6);
    }
    #[tokio::test]
    async fn pagination_is_retained_until_final_page_and_does_not_release_other_results() {
        let mut c = fake(vec![
            page("one", json!("one:1")),
            page("two", json!("two:1")),
            page("one", Value::Null),
            json!({"data":{"released":true}}),
        ]);
        let mut d = Delivery::default();
        let first = call(&mut c, &mut d, "winproc_list_processes", json!({"limit":1}))
            .await
            .unwrap();
        call(&mut c, &mut d, "winproc_list_processes", json!({"limit":1}))
            .await
            .unwrap();
        let action = &first["data"]["next_action"];
        assert_eq!(action["tool"], "winproc_read_capture");
        assert_eq!(c.calls.len(), 2);
        let last = call(
            &mut c,
            &mut d,
            action["tool"].as_str().unwrap(),
            action["arguments"].clone(),
        )
        .await
        .unwrap();
        assert_eq!(last["data"]["capture_retained"], false);
        assert!(d.pages.contains_key("two"));
        assert_eq!(c.calls[3].1["capture_id"], "one");
    }
    #[tokio::test]
    async fn retained_results_and_confirmation_are_not_consumed_or_replayed() {
        let mut c = fake(vec![
            page("kept", Value::Null),
            json!({"data":{"operation_id":"o","state":"pending_confirmation"}}),
        ]);
        let mut d = Delivery::default();
        let r = call(
            &mut c,
            &mut d,
            "winproc_list_processes",
            json!({"retain_capture":true}),
        )
        .await
        .unwrap();
        assert_eq!(r["data"]["capture_retained"], true);
        assert!(c.calls[0].1.get("retain_capture").is_none());
        let r = call(
            &mut c,
            &mut d,
            "winproc_get_operation",
            json!({"operation_id":"o"}),
        )
        .await
        .unwrap();
        assert_eq!(r["data"]["state"], "pending_confirmation");
        assert!(r["data"]["next_action"].is_null());
        assert_eq!(c.calls.len(), 2);
    }
    #[tokio::test]
    async fn nonblocking_wait_returns_exact_continuation_and_keeps_selection() {
        let mut c = fake(vec![json!({"data":{"job_id":"j","state":"running"}})]);
        let mut d = Delivery::default();
        let r = call(
            &mut c,
            &mut d,
            "winproc_start_capture",
            json!({"kind":"file_users","wait_ms":0,"read":{"extension":".log"}}),
        )
        .await
        .unwrap();
        assert_eq!(c.calls.len(), 1);
        assert_eq!(r["data"]["next_action"]["tool"], "winproc_get_job");
        assert_eq!(
            r["data"]["next_action"]["arguments"]["read"]["extension"],
            ".log"
        );
    }
    #[tokio::test]
    async fn read_error_preserves_retained_data_and_metric_defaults_are_explicit() {
        let mut c = fake(vec![
            json!({"error":{"code":"INVALID_ARGUMENT"}}),
            json!({"data":{"samples":[]}}),
        ]);
        let mut d = Delivery::default();
        let r = call(
            &mut c,
            &mut d,
            "winproc_read_capture",
            json!({"capture_id":"c","projection":["invalid"]}),
        )
        .await
        .unwrap();
        assert_eq!(r["error"]["code"], "INVALID_ARGUMENT");
        call(
            &mut c,
            &mut d,
            "winproc_get_metrics",
            json!({"metric_ids":["cpu_average_percent"]}),
        )
        .await
        .unwrap();
        assert_eq!(c.calls.len(), 2);
        assert_eq!(c.calls[1].1["source"], "live");
        assert_eq!(c.calls[1].1["targets"], json!([{"system":true}]));
    }
    #[tokio::test]
    async fn paginated_job_is_consumed_only_after_its_last_page() {
        let mut c = fake(vec![
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
            page("derived", json!("derived:1")),
            page("derived", Value::Null),
            json!({"data":{"released":true}}),
            json!({"data":{"released":true}}),
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
        ]);
        let mut d = Delivery::default();
        let first = call(&mut c, &mut d, "winproc_get_job", json!({"job_id":"j"}))
            .await
            .unwrap();
        assert!(!d.consumed_jobs.contains_key("j"));
        let next = &first["data"]["next_action"];
        call(
            &mut c,
            &mut d,
            next["tool"].as_str().unwrap(),
            next["arguments"].clone(),
        )
        .await
        .unwrap();
        assert!(d.consumed_jobs.contains_key("j"));
        let replay = call(&mut c, &mut d, "winproc_get_job", json!({"job_id":"j"}))
            .await
            .unwrap();
        assert_eq!(replay["data"]["result_consumed"], true);
        assert_eq!(c.calls.len(), 6);
    }
    #[tokio::test]
    async fn selection_failure_keeps_source_and_cleanup_failure_closes_connection() {
        let mut d = Delivery::default();
        let mut c = fake(vec![
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
            json!({"error":{"code":"INVALID_ARGUMENT"}}),
        ]);
        let r = call(&mut c, &mut d, "winproc_get_job", json!({"job_id":"j"}))
            .await
            .unwrap();
        assert_eq!(r["data"]["capture_retained"], true);
        assert_eq!(r["data"]["retained_capture_ids"], json!(["source"]));
        assert_eq!(c.calls.len(), 2);
        let mut c = fake(vec![
            page("c", Value::Null),
            json!({"error":{"code":"INTERNAL_ERROR"}}),
        ]);
        assert!(
            call(&mut c, &mut d, "winproc_list_processes", json!({}))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn explicit_release_and_log_close_forget_only_the_owned_chain() {
        let mut logged = page("log-child", json!("log-child:1"));
        logged["log_ref"] = json!("log");
        let mut c = fake(vec![
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"source"}}}),
            page("derived", json!("derived:1")),
            logged,
            json!({"data":{"released":true}}),
            json!({"data":{"released":true}}),
            json!({"data":{"closed":true}}),
        ]);
        let mut d = Delivery::default();
        call(&mut c, &mut d, "winproc_get_job", json!({"job_id":"j"}))
            .await
            .unwrap();
        call(
            &mut c,
            &mut d,
            "winproc_read_capture",
            json!({"capture_id":"log-child"}),
        )
        .await
        .unwrap();
        call(
            &mut c,
            &mut d,
            "winproc_release_capture",
            json!({"capture_id":"derived"}),
        )
        .await
        .unwrap();
        assert_eq!(d.pages.len(), 1);
        assert!(d.pages.contains_key("log-child"));
        assert_eq!(c.calls[4].1["capture_id"], "source");
        call(
            &mut c,
            &mut d,
            "winproc_close_log",
            json!({"log_ref":"log"}),
        )
        .await
        .unwrap();
        assert!(d.pages.is_empty());
        assert!(d.consumed_jobs.contains_key("j"));
    }
    #[tokio::test]
    async fn invalidated_backend_pages_cannot_grow_bridge_bookkeeping_unbounded() {
        let mut c = fake(
            (0..20)
                .map(|i| page(&format!("page-{i}"), json!(format!("page-{i}:1"))))
                .collect(),
        );
        let mut d = Delivery::default();
        for _ in 0..20 {
            call(&mut c, &mut d, "winproc_list_processes", json!({"limit":1}))
                .await
                .unwrap();
        }
        assert_eq!(d.pages.len(), 8);
        assert_eq!(c.calls.len(), 20);
    }
}
