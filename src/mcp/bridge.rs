//! Standard MCP stdio endpoint; Windows collection remains in the application.
use super::{
    catalog,
    transport::{Client, error},
};
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    model::*,
    service::{RequestContext, RoleServer},
};
use serde_json::Value;
use std::{borrow::Cow, time::Duration};
use tokio::{sync::Mutex, time::timeout};

#[derive(Default)]
struct Bridge {
    client: Mutex<Option<Connection>>,
    raw: bool,
}
struct Connection {
    client: Client,
    delivery: super::managed::Delivery,
}
impl ServerHandler for Bridge {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(if self.raw { "Connects to WinProc TUI in the same Windows logon session. The first valid tool call starts the application in a new visible Windows Terminal window if absent and MCP is enabled in its saved configuration. Startup may take up to 12 seconds. The application remains open after disconnect. No automatic elevation or headless fallback. For asynchronous tools use wait_ms:5000; capture-producing tools also accept read to return a completed first page in one call. Continue a running job with winproc_get_job and the same options. For application operations wait_ms also stops at local confirmation. Avoid preliminary status calls unless status itself is needed. Use projection and larger bounded pages for bulk reads; follow next_cursor only when non-null." } else { "WinProc TUI investigation tools. Call the tool matching the user request directly; no preliminary status call is needed. Jobs and operations wait up to 5 seconds by default and captures include their first result page. Completed final pages are automatically released: do not call release_capture when capture_retained is false. If data.next_action is non-null, invoke exactly its tool and arguments to continue; null means no automatic follow-up. Pending local confirmation requires a human TUI decision, never automated approval. Use retain_capture:true only for repeated inspection of the same frozen data. Metrics default to the live system; a log_ref selects that saved log unless source is explicit. All identity, revision, idempotency and confirmation requirements remain in force. The first valid call can start the normal visible application; it stays open after disconnect." })
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|request| request.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "Tool catalog has no continuation cursor",
                None,
            ));
        }
        Ok(ListToolsResult {
            tools: catalog::tools_for_client(self.raw),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !catalog::available(&request.name) {
            return Err(ErrorData::invalid_params("Unknown tool", None));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        match catalog::validate(&request.name, &arguments) {
            None => return Err(ErrorData::invalid_params("Unknown tool", None)),
            Some(false) => {
                return Ok(CallToolResult::structured_error(catalog::validation_error(
                    &request.name,
                    &arguments,
                ))
                .into());
            }
            Some(true) => {}
        }
        let mut client = match self.client.try_lock() {
            Ok(client) => client,
            Err(_) => {
                return Ok(CallToolResult::structured_error(error(
                    "BUSY",
                    "Another request is in progress on this bridge",
                    true,
                ))
                .into());
            }
        };
        if client.is_none() {
            match super::startup::connect(|| context.ct.is_cancelled()).await {
                Ok(connection) => {
                    *client = Some(Connection {
                        client: connection,
                        delivery: Default::default(),
                    })
                }
                Err(error) => return Ok(CallToolResult::structured_error(error).into()),
            }
        }
        if context.ct.is_cancelled() {
            return Ok(CallToolResult::structured_error(error(
                "CANCELLED",
                "Request cancelled before dispatch",
                false,
            ))
            .into());
        }
        let connection = client.as_mut().expect("connected");
        let call = async {
            if self.raw {
                super::completion::call(&mut connection.client, &request.name, arguments).await
            } else {
                super::managed::call(
                    &mut connection.client,
                    &mut connection.delivery,
                    &request.name,
                    arguments,
                )
                .await
            }
        };
        let result = tokio::select! {
            _ = context.ct.cancelled() => Err(()),
            result = timeout(Duration::from_secs(10), call) => match result { Ok(Ok(value)) => Ok(value), _ => Err(()) },
        };
        let value = match result {
            Ok(value) => value,
            Err(()) => {
                *client = None; // Drop the stream: never replay an uncertain mutation.
                error(
                    "OUTCOME_UNKNOWN",
                    "Connection closed or request cancelled; inspect operation status before submitting a new mutation",
                    false,
                )
            }
        };
        let result = if value.get("error").is_some() {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        };
        if serde_json::to_vec(&result).map_or(true, |bytes| {
            bytes.len() > super::protocol::MAX_FRAME_BYTES - 1024
        }) {
            return Ok(CallToolResult::structured_error(error(
                "RESOURCE_LIMIT",
                "Response exceeds the encoded message limit; request a smaller page",
                false,
            ))
            .into());
        }
        Ok(result.into())
    }
}

pub(crate) fn run(raw: bool) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let service = Bridge {
                raw,
                ..Default::default()
            }
            .serve(
                rmcp::transport::async_rw::AsyncRwTransport::new_server(
                    tokio::io::stdin(),
                    tokio::io::stdout(),
                )
                .with_max_line_length(super::protocol::MAX_FRAME_BYTES),
            )
            .await?;
            service.waiting().await?;
            Ok(())
        })
}
