//! Application-owned serialized operations. Transport disconnect only cancels pending work.
use super::transport::error;
use crate::model::ProcessIdentity;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub(crate) struct GraphTargets {
    pub identities: HashMap<String, ProcessIdentity>,
    pub revision: Option<String>,
}
#[derive(Clone)]
pub(crate) struct Action {
    pub id: String,
    pub connection: String,
    pub method: String,
    pub args: Value,
    pub identity: Option<ProcessIdentity>,
    pub created: Instant,
    pub graph: GraphTargets,
}
struct Entry {
    signature: Value,
    result: Value,
    completed: Option<Instant>,
}
#[derive(Default)]
pub(crate) struct Operations {
    entries: HashMap<String, Entry>,
    pub pending: Option<Action>,
    pub active: Option<String>,
}
pub(crate) type Shared = Arc<Mutex<Operations>>;
impl Operations {
    pub fn submit(
        &mut self,
        connection: &str,
        method: &str,
        args: &Value,
        identity: Option<ProcessIdentity>,
    ) -> Value {
        self.entries.retain(|_, e| {
            e.completed
                .is_none_or(|t| t.elapsed() < Duration::from_secs(86400))
        });
        let id = args["operation_id"].as_str().unwrap().to_owned();
        let signature = json!({"method":method,"args":args});
        if let Some(entry) = self.entries.get(&id) {
            return if entry.signature == signature {
                entry.result.clone()
            } else {
                error(
                    "STATE_CONFLICT",
                    "Operation ID was already used with different arguments",
                    false,
                )
            };
        }
        if self.active.is_some() || self.entries.len() >= 1024 {
            return error("BUSY", "Operation capacity reached", true);
        }
        let result = json!({"operation_id":id,"state":"pending"});
        self.entries.insert(
            id.clone(),
            Entry {
                signature,
                result: result.clone(),
                completed: None,
            },
        );
        self.active = Some(id.clone());
        self.pending = Some(Action {
            id,
            connection: connection.into(),
            method: method.into(),
            args: args.clone(),
            identity,
            created: Instant::now(),
            graph: GraphTargets::default(),
        });
        result
    }
    pub fn submit_graph(
        &mut self,
        connection: &str,
        method: &str,
        args: &Value,
        graph: GraphTargets,
    ) -> Value {
        let id = args["operation_id"].as_str().unwrap();
        let fresh = !self.entries.contains_key(id);
        let result = self.submit(connection, method, args, None);
        if fresh
            && let Some(action) = self.pending.as_mut()
            && action.id == id
        {
            action.graph = graph;
        }
        result
    }
    pub fn running(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.result = json!({"operation_id":id,"state":"running"});
        }
        self.pending = None;
    }
    pub fn get(&self, id: &str) -> Value {
        self.entries
            .get(id)
            .filter(|e| {
                e.completed
                    .is_none_or(|t| t.elapsed() < Duration::from_secs(86400))
            })
            .map(|e| e.result.clone())
            .unwrap_or_else(|| {
                error(
                    "OPERATION_UNKNOWN",
                    "Operation is unknown or expired",
                    false,
                )
            })
    }
    pub fn finish(&mut self, id: &str, result: Value) {
        if self.active.as_deref() != Some(id) {
            return;
        }
        if let Some(entry) = self.entries.get_mut(id) {
            entry.result = json!({"operation_id":id,"state":if result.get("error").is_some(){"failed"}else{"succeeded"},"result":result});
            entry.completed = Some(Instant::now());
        }
        self.pending = None;
        self.active = None;
    }
    pub fn confirming(&mut self, id: &str, description: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.result =
                json!({"operation_id":id,"state":"pending_confirmation","description":description});
        }
    }
    pub fn cancel_connection(&mut self, connection: Option<&str>) {
        if let Some(action) = self.pending.clone()
            && connection.is_none_or(|c| c == action.connection)
        {
            self.finish(
                &action.id,
                error(
                    "CANCELLED",
                    "Requesting connection closed or MCP disabled",
                    false,
                ),
            );
        }
    }
}
pub(crate) fn is_action(method: &str) -> bool {
    matches!(
        method,
        "winproc_add_graphs"
            | "winproc_export_graphs"
            | "winproc_update_tracking"
            | "winproc_update_profile"
            | "winproc_start_recording"
            | "winproc_stop_recording"
            | "winproc_set_dotnet_collection"
    )
}
