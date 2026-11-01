//! Committed tool contracts shared by the bridge and backend validation.
use rmcp::model::Tool;
use serde_json::Value;
use std::sync::LazyLock;

pub(crate) static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../docs/schemas/mcp-tools.json"))
        .expect("committed tool catalog")
});
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
