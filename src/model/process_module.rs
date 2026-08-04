use chrono::{DateTime, Local};

use crate::model::InfoValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessModuleEntry {
    pub(crate) path: String,
    pub(crate) dll_name: String,
    pub(crate) directory: String,
    pub(crate) company_name: InfoValue,
    pub(crate) product_version: InfoValue,
    pub(crate) file_version: InfoValue,
    pub(crate) modified: InfoValue,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessModulesReport {
    pub(crate) pid: u32,
    pub(crate) process_name: String,
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) entries: Vec<ProcessModuleEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessModulesError {
    AccessDenied,
    ProcessExited,
    IdentityChanged,
    QueryFailed,
}

impl ProcessModulesError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::AccessDenied => "<access denied>",
            Self::ProcessExited => "<exited>",
            Self::IdentityChanged => "<process changed>",
            Self::QueryFailed => "<query failed>",
        }
    }
}
