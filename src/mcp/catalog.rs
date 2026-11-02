//! Committed tool contracts shared by the bridge and backend validation.
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::{Map, Value};
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
}
