//! Committed tool contracts shared by the bridge and backend validation.
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
struct ToolDefinition {
    #[serde(rename = "outputDataSchema")]
    output_data_schema: Option<Map<String, Value>>,
    #[serde(flatten)]
    tool: Tool,
}

// Source files are embedded in the executable; discovery needs no file/network I/O.
pub(crate) static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    expand_catalog(
        include_str!("../../docs/schemas/mcp-tools.json"),
        include_str!("../../docs/schemas/mcp-output-envelope.schema.json"),
    )
});

fn expand_catalog(definitions: &str, envelope: &str) -> Vec<Tool> {
    let definitions: Vec<ToolDefinition> =
        serde_json::from_str(definitions).expect("committed tool definitions");
    let envelope: Value = serde_json::from_str(envelope).expect("committed output envelope");
    definitions
        .into_iter()
        .map(|mut definition| {
            assert!(
                definition.tool.output_schema.is_none(),
                "tool definitions use outputDataSchema, not a duplicated outputSchema"
            );
            let mut output = envelope.clone();
            let data = output
                .pointer_mut("/oneOf/0/properties/data")
                .expect("common output envelope has a success data schema");
            if let Some(schema) = definition.output_data_schema {
                *data = Value::Object(schema);
            }
            definition.tool.output_schema = Some(Arc::new(
                output
                    .as_object()
                    .expect("output envelope is an object")
                    .clone(),
            ));
            definition.tool
        })
        .collect()
}
static VALIDATORS: LazyLock<Vec<jsonschema::Validator>> = LazyLock::new(|| {
    TOOLS
        .iter()
        .map(|tool| {
            jsonschema::options()
                .should_validate_formats(true)
                .build(&Value::Object((*tool.input_schema).clone()))
                .expect("committed input schema")
        })
        .collect()
});
pub(crate) fn validate(name: &str, arguments: &Value) -> Option<bool> {
    TOOLS
        .iter()
        .position(|tool| tool.name == name)
        .map(|i| VALIDATORS[i].is_valid(arguments))
}

// The catalog and adapter list are checked for parity in tests.
pub(crate) fn available_tools() -> Vec<Tool> {
    TOOLS
        .iter()
        .filter(|tool| available(&tool.name))
        .cloned()
        .collect()
}
pub(crate) fn tools_for_client(raw: bool) -> Vec<Tool> {
    let mut tools = available_tools();
    if raw {
        for tool in &mut tools {
            let schema = Arc::make_mut(&mut tool.input_schema);
            if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                if let Some(wait) = properties.get_mut("wait_ms") {
                    wait["default"] = serde_json::json!(0);
                    wait["description"] = serde_json::json!(
                        "Raw mode defaults to 0; opt into at most 5000 ms of completion waiting."
                    );
                }
                if let Some(retain) = properties.get_mut("retain_capture") {
                    retain["default"] = serde_json::json!(true);
                    retain["description"] = serde_json::json!(
                        "Raw mode always retains captures; release them explicitly."
                    );
                }
                if let Some(read) = properties.get_mut("read") {
                    read.as_object_mut().unwrap().remove("default");
                    if let Some(limit) = read.pointer_mut("/properties/limit") {
                        limit["default"] = serde_json::json!(100);
                    }
                }
                if let Some(limit) = properties.get_mut("limit") {
                    limit["default"] = serde_json::json!(100);
                }
            }
            tool.description = Some(std::borrow::Cow::Owned(format!(
                "Raw mode: asynchronous by default; captures remain retained until explicit release. {}",
                tool.description.as_deref().unwrap_or("")
                    .replace("Waits up to 5 seconds by default; use wait_ms:0 for an immediate handle.", "Default wait_ms is 0; opt into wait_ms:5000 for completion.")
                    .replace("Returns the first page in data.result_page automatically. Optional read selects rows/fields.", "Use read to opt into a first result page.")
                    .replace(" Final pages are automatically released by default. Follow data.next_action only when non-null; use retain_capture:true for repeated reads.", "")
            )));
        }
    }
    tools
}

pub(crate) fn apply_defaults(method: &str, args: &mut Value) {
    if !args.is_object() {
        return;
    }
    if matches!(
        method,
        "winproc_list_processes" | "winproc_get_metrics" | "winproc_get_history"
    ) && args.get("source").is_none()
    {
        args["source"] = json!(if args.get("log_ref").is_some() {
            "log"
        } else {
            "live"
        });
    }
    if method == "winproc_get_metrics" && args.get("targets").is_none() {
        args["targets"] = json!([{"system":true}]);
    }
}

pub(crate) fn available(name: &str) -> bool {
    matches!(
        name,
        "winproc_get_status"
            | "winproc_get_system_info"
            | "winproc_list_metrics"
            | "winproc_list_processes"
            | "winproc_get_metrics"
            | "winproc_get_history"
            | "winproc_start_capture"
            | "winproc_get_job"
            | "winproc_read_capture"
            | "winproc_cancel_job"
            | "winproc_release_capture"
            | "winproc_get_process_info"
            | "winproc_wait_condition"
            | "winproc_list_logs"
            | "winproc_open_log"
            | "winproc_close_log"
            | "winproc_get_tracking"
            | "winproc_update_tracking"
            | "winproc_list_profiles"
            | "winproc_update_profile"
            | "winproc_get_recording"
            | "winproc_start_recording"
            | "winproc_stop_recording"
            | "winproc_get_dotnet_collection"
            | "winproc_set_dotnet_collection"
            | "winproc_get_operation"
    )
}

pub(crate) fn validation_error(name: &str, args: &Value) -> Value {
    let mut value = super::transport::error(
        "INVALID_ARGUMENT",
        "Arguments must satisfy the indicated input schema constraint",
        false,
    );
    if let Some(i) = TOOLS.iter().position(|t| t.name == name)
        && let Some(e) = VALIDATORS[i].iter_errors(args).next()
    {
        value["error"]["field"] = serde_json::json!(e.instance_path().to_string());
        value["error"]["constraint"] = serde_json::json!(e.schema_path().to_string());
        // Only committed schema values are returned, never the rejected input.
        if let Some(expected) = serde_json::to_value(&*TOOLS[i].input_schema)
            .ok()
            .and_then(|s| s.pointer(&e.schema_path().to_string()).cloned())
        {
            value["error"]["expected"] = expected;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn expanded_catalog_preserves_tool_definitions_and_common_output_contract() {
        let definitions: Vec<Value> =
            serde_json::from_str(include_str!("../../docs/schemas/mcp-tools.json")).unwrap();
        let envelope: Value = serde_json::from_str(include_str!(
            "../../docs/schemas/mcp-output-envelope.schema.json"
        ))
        .unwrap();
        assert_eq!(definitions.len(), TOOLS.len());
        for (definition, tool) in definitions.iter().zip(TOOLS.iter()) {
            assert!(definition.get("outputSchema").is_none());
            let published = serde_json::to_value(tool).unwrap();
            for (key, value) in definition.as_object().unwrap() {
                if key != "outputDataSchema" {
                    assert_eq!(&published[key], value, "{key}");
                }
            }
            assert!(published.get("outputDataSchema").is_none());
            assert_eq!(published["outputSchema"], envelope);
        }
    }

    #[test]
    fn per_tool_data_schema_is_isolated_and_keeps_common_errors() {
        let definitions = json!([
            {"name":"typed", "inputSchema":{"type":"object"},
             "outputDataSchema":{"type":"object", "required":["count"],
                "properties":{"count":{"type":"integer"}}, "additionalProperties":false}},
            {"name":"generic", "inputSchema":{"type":"object"}}
        ]);
        let tools = expand_catalog(
            &definitions.to_string(),
            include_str!("../../docs/schemas/mcp-output-envelope.schema.json"),
        );
        let validators: Vec<_> = tools
            .iter()
            .map(|tool| {
                jsonschema::validator_for(&Value::Object(
                    (**tool.output_schema.as_ref().unwrap()).clone(),
                ))
                .unwrap()
            })
            .collect();
        let mut success = json!({"api_version":"1.0", "backend_instance_id":"test",
            "source":"live", "status":"complete", "warnings":[], "data":{"count":2}});
        assert!(validators[0].is_valid(&success));
        success["data"] = json!({"count":"2"});
        assert!(!validators[0].is_valid(&success));
        success["data"] = json!({});
        assert!(!validators[0].is_valid(&success));
        assert!(validators[1].is_valid(&success));
        for validator in validators {
            assert!(
                validator.is_valid(&json!({"error":{"code":"INVALID_ARGUMENT",
                "message":"Invalid input", "retryable":false}}))
            );
            assert!(!validator.is_valid(&json!({"error":{"code":"INVALID_ARGUMENT"}})));
            let mut invalid = success.clone();
            invalid.as_object_mut().unwrap().remove("source");
            assert!(!validator.is_valid(&invalid));
        }
    }

    #[test]
    #[should_panic(expected = "not a duplicated outputSchema")]
    fn definitions_reject_a_duplicated_output_envelope() {
        expand_catalog(
            r#"[{"name":"bad","inputSchema":{"type":"object"},
            "outputSchema":{"type":"object"}}]"#,
            include_str!("../../docs/schemas/mcp-output-envelope.schema.json"),
        );
    }

    #[test]
    fn catalog_rejects_unknown_fields_and_enforces_source_and_identity() {
        assert_eq!(TOOLS.len(), 26);
        assert_eq!(available_tools().len(), TOOLS.len());
        for tool in TOOLS.iter() {
            assert_eq!(
                tool.output_schema.as_ref().unwrap().get("type"),
                Some(&json!("object"))
            );
        }
        assert_eq!(validate("winproc_get_status", &json!({})), Some(true));
        assert_eq!(
            validate("winproc_get_status", &json!({"extra":true})),
            Some(false)
        );
        assert_eq!(
            validate(
                "winproc_get_metrics",
                &json!({"source":"log","targets":[{"system":true}],"metric_ids":["cpu_percent"]})
            ),
            Some(false)
        );
        assert_eq!(
            validate("winproc_start_capture", &json!({"kind":"modules"})),
            Some(false)
        );
        assert_eq!(
            validate(
                "winproc_update_tracking",
                &json!({"action":"add","process_name":"x.exe","process_ref":"ref","operation_id":"op","expected_revision":"1"})
            ),
            Some(false)
        );
        assert_eq!(validate("winproc_unknown", &json!({})), None);
    }
    #[test]
    fn bounded_completion_and_large_pages_validate_without_weakening_other_inputs() {
        for name in [
            "winproc_start_capture",
            "winproc_get_job",
            "winproc_get_process_info",
            "winproc_list_logs",
            "winproc_open_log",
            "winproc_wait_condition",
            "winproc_get_operation",
            "winproc_update_tracking",
            "winproc_update_profile",
            "winproc_start_recording",
            "winproc_stop_recording",
            "winproc_set_dotnet_collection",
        ] {
            let tool = TOOLS.iter().find(|t| t.name == name).unwrap();
            assert_eq!(tool.input_schema["properties"]["wait_ms"]["maximum"], 5000);
        }
        assert_eq!(
            validate(
                "winproc_start_capture",
                &json!({"kind":"file_users","wait_ms":5000,"read":{"projection":["path"],"distinct_paths":true,"limit":5000}})
            ),
            Some(true)
        );
        for args in [
            json!({"kind":"file_users","wait_ms":5001}),
            json!({"kind":"file_users","read":{"unknown":true}}),
            json!({"kind":"file_users","read":{"distinct_paths":true}}),
        ] {
            assert_eq!(validate("winproc_start_capture", &args), Some(false));
        }
        assert_eq!(
            validate(
                "winproc_read_capture",
                &json!({"capture_id":"c","extension":".log","owner_name":"chrome.exe","limit":5000})
            ),
            Some(true)
        );
        assert_eq!(
            validate(
                "winproc_read_capture",
                &json!({"capture_id":"c","extension":".log/secret"})
            ),
            Some(false)
        );
        assert_eq!(
            validate("winproc_get_status", &json!({"wait_ms":5000})),
            Some(false)
        );
    }
    #[test]
    fn metric_choices_and_raw_defaults_match_runtime_contracts() {
        let expected: std::collections::HashSet<_> = super::super::metrics::SYSTEM_METRICS
            .iter()
            .chain(super::super::metrics::PROCESS_METRICS)
            .copied()
            .collect();
        for tool in TOOLS.iter().filter(|t| {
            matches!(
                t.name.as_ref(),
                "winproc_get_metrics" | "winproc_get_history" | "winproc_wait_condition"
            )
        }) {
            let schema = serde_json::to_value(&*tool.input_schema).unwrap();
            let choices = schema
                .pointer("/properties/metric_ids/items/enum")
                .or_else(|| schema.pointer("/properties/metric_id/enum"))
                .unwrap()
                .as_array()
                .unwrap();
            assert_eq!(
                choices
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<std::collections::HashSet<_>>(),
                expected
            );
        }
        assert_eq!(
            validate(
                "winproc_get_metrics",
                &json!({"metric_ids":["cpu_average_percent"]})
            ),
            Some(true)
        );
        assert_eq!(
            validate("winproc_get_metrics", &json!({"metric_ids":["invented"]})),
            Some(false)
        );
        let error = validation_error("winproc_get_metrics", &json!({}));
        assert_eq!(error["error"]["expected"], json!(["metric_ids"]));
        let raw = tools_for_client(true);
        let ready = tools_for_client(false);
        assert_eq!(raw.len(), 26);
        assert_eq!(ready.len(), 26);
        for (raw, ready) in raw.iter().zip(&ready) {
            if ready.input_schema["properties"].get("wait_ms").is_some() {
                assert_eq!(ready.input_schema["properties"]["wait_ms"]["default"], 5000);
                assert_eq!(raw.input_schema["properties"]["wait_ms"]["default"], 0);
            }
        }
    }
    #[test]
    fn every_tool_description_contains_a_schema_valid_example() {
        for tool in TOOLS.iter() {
            let tail = tool
                .description
                .as_deref()
                .unwrap()
                .split("Example arguments: ")
                .nth(1)
                .expect("tool example");
            let args = serde_json::Deserializer::from_str(tail)
                .into_iter::<Value>()
                .next()
                .unwrap()
                .unwrap();
            assert_eq!(validate(&tool.name, &args), Some(true), "{}", tool.name);
        }
        let mut malformed = Value::Null;
        apply_defaults("winproc_get_metrics", &mut malformed);
        assert!(malformed.is_null());
    }
}
