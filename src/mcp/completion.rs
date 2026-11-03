//! Opt-in completion in the bridge, never in the shared application dispatcher.
use super::transport::Client;
use serde_json::{Value, json};
use std::{future::Future, io, time::Duration};
use tokio::time::{Instant, sleep};

pub(crate) trait Caller {
    fn call(&mut self, method: &str, args: Value)
    -> impl Future<Output = io::Result<Value>> + Send;
}
impl Caller for Client {
    async fn call(&mut self, method: &str, args: Value) -> io::Result<Value> {
        Client::call(self, method, args).await
    }
}

pub(crate) async fn call(
    client: &mut (impl Caller + Send),
    method: &str,
    mut args: Value,
) -> io::Result<Value> {
    args.as_object_mut().unwrap().remove("retain_capture");
    super::catalog::apply_defaults(method, &mut args);
    let wait = args
        .as_object_mut()
        .unwrap()
        .remove("wait_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let read = args.as_object_mut().unwrap().remove("read");
    let deadline = Instant::now() + Duration::from_millis(wait);
    let mut value = client.call(method, args).await?;
    let (getter, key, id) = if let Some(id) = value["data"]["job_id"].as_str() {
        ("winproc_get_job", "job_id", id.to_owned())
    } else if let Some(id) = value["data"]["operation_id"].as_str() {
        ("winproc_get_operation", "operation_id", id.to_owned())
    } else {
        return Ok(value);
    };
    while matches!(value["data"]["state"].as_str(), Some("running" | "pending"))
        && Instant::now() < deadline
    {
        sleep(Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())))
            .await;
        if Instant::now() >= deadline {
            break;
        }
        value = client.call(getter, json!({(key): id})).await?;
    }
    // A local confirmation is a terminal state for this bounded wait, never an approval.
    // Keep job failure and successful empty results in their existing outcome envelope.
    if value["data"]["state"] == "succeeded"
        && let Some(mut query) = read
        && let Some(capture) = value["data"]["result"]["capture_id"].as_str()
    {
        query["capture_id"] = json!(capture);
        let page = client.call("winproc_read_capture", query).await?;
        value["data"]["result_page"] = page;
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
    #[tokio::test]
    async fn completed_job_returns_selected_first_page_without_replaying_start() {
        let mut c = fake(vec![
            json!({"data":{"job_id":"j","state":"running"}}),
            json!({"data":{"job_id":"j","state":"succeeded","result":{"capture_id":"c","status":"partial"}}}),
            json!({"status":"partial","data":{"capture_id":"derived","rows":[{"path":"x"}],"next_cursor":null}}),
        ]);
        let result = call(
            &mut c,
            "winproc_start_capture",
            json!({"kind":"file_users","wait_ms":5000,"read":{"projection":["path"],"limit":5000}}),
        )
        .await
        .unwrap();
        assert_eq!(
            c.calls.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            vec![
                "winproc_start_capture",
                "winproc_get_job",
                "winproc_read_capture"
            ]
        );
        assert_eq!(c.calls[0].1, json!({"kind":"file_users"}));
        assert_eq!(c.calls[2].1["capture_id"], "c");
        assert_eq!(result["data"]["result_page"]["status"], "partial");
        assert_eq!(
            result["data"]["result_page"]["data"]["rows"][0]["path"],
            "x"
        );
    }
    #[tokio::test]
    async fn default_is_nonblocking_and_wait_deadline_keeps_job_running() {
        let mut c = fake(vec![json!({"data":{"job_id":"j","state":"running"}})]);
        let result = call(
            &mut c,
            "winproc_start_capture",
            json!({"kind":"file_users","read":{}}),
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["state"], "running");
        assert_eq!(c.calls.len(), 1);
        let mut c = fake(vec![json!({"data":{"job_id":"j","state":"running"}})]);
        let result = call(&mut c, "winproc_get_job", json!({"job_id":"j","wait_ms":1}))
            .await
            .unwrap();
        assert_eq!(result["data"]["state"], "running");
        assert_eq!(c.calls.len(), 1);
    }
    #[tokio::test]
    async fn confirmation_and_failure_are_returned_without_more_work() {
        for state in ["pending_confirmation", "failed"] {
            let mut c = fake(vec![json!({"data":{"operation_id":"o","state":state}})]);
            let result = call(
                &mut c,
                "winproc_get_operation",
                json!({"operation_id":"o","wait_ms":5000}),
            )
            .await
            .unwrap();
            assert_eq!(result["data"]["state"], state);
            assert_eq!(c.calls.len(), 1);
        }
        let mut c = fake(vec![
            json!({"data":{"operation_id":"o","state":"pending"}}),
            json!({"data":{"operation_id":"o","state":"succeeded"}}),
        ]);
        call(
            &mut c,
            "winproc_update_tracking",
            json!({"operation_id":"o","wait_ms":5000}),
        )
        .await
        .unwrap();
        assert_eq!(c.calls[1].0, "winproc_get_operation");
    }
    #[tokio::test]
    async fn errors_cached_reads_and_cancelled_wait_never_replay() {
        for reply in [
            json!({"error":{"code":"BUSY"}}),
            json!({"data":{"values":{}}}),
        ] {
            let mut c = fake(vec![reply.clone()]);
            assert_eq!(
                call(&mut c, "winproc_get_metrics", json!({}))
                    .await
                    .unwrap(),
                reply
            );
            assert_eq!(c.calls.len(), 1);
        }
        struct Blocking {
            calls: usize,
        }
        impl Caller for Blocking {
            async fn call(&mut self, _: &str, _: Value) -> io::Result<Value> {
                self.calls += 1;
                std::future::pending().await
            }
        }
        let mut c = Blocking { calls: 0 };
        let result = tokio::time::timeout(
            Duration::from_millis(2),
            call(&mut c, "winproc_start_capture", json!({"wait_ms":5000})),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(c.calls, 1);
    }
}
