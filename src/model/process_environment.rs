use chrono::{DateTime, Local};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessEnvironmentEntry {
    pub(crate) name: String,
    pub(crate) value: String,
}

impl ProcessEnvironmentEntry {
    pub(crate) fn raw(&self) -> String {
        format!("{}={}", self.name, self.value)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessEnvironmentReport {
    pub(crate) pid: u32,
    pub(crate) process_name: String,
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) entries: Vec<ProcessEnvironmentEntry>,
    pub(crate) malformed_entries: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessEnvironmentError {
    AccessDenied,
    ProcessExited,
    IdentityChanged,
    NotAvailable,
    UnsupportedArchitecture,
    ReadFailed,
    TooLarge,
}

impl ProcessEnvironmentError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::AccessDenied => "<access denied>",
            Self::ProcessExited => "<exited>",
            Self::IdentityChanged => "<process changed>",
            Self::NotAvailable => "<not available>",
            Self::UnsupportedArchitecture => "<unsupported architecture>",
            Self::ReadFailed => "<read failed>",
            Self::TooLarge => "<too large>",
        }
    }
}
