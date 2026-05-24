#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ProcessRow {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) start_time: Option<u64>,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) private_bytes: Option<u64>,
    pub(crate) workset_bytes: Option<u64>,
    pub(crate) workset_private_bytes: Option<u64>,
    pub(crate) workset_shareable_bytes: Option<u64>,
    pub(crate) workset_shared_bytes: Option<u64>,
    pub(crate) thread_count: Option<u64>,
    pub(crate) handle_count: Option<u64>,
    pub(crate) user_object_count: Option<u64>,
    pub(crate) gdi_object_count: Option<u64>,
    pub(crate) gpu_percent: Option<f64>,
    pub(crate) gpu_dedicated_bytes: Option<u64>,
    pub(crate) gpu_shared_bytes: Option<u64>,
    pub(crate) dotnet_heap_bytes: Option<u64>,
    pub(crate) io_read_bytes_per_sec: Option<u64>,
    pub(crate) io_write_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessExtraMetrics {
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) private_bytes: Option<u64>,
    pub(crate) workset_bytes: Option<u64>,
    pub(crate) workset_private_bytes: Option<u64>,
    pub(crate) workset_shareable_bytes: Option<u64>,
    pub(crate) workset_shared_bytes: Option<u64>,
    pub(crate) thread_count: Option<u64>,
    pub(crate) handle_count: Option<u64>,
    pub(crate) user_object_count: Option<u64>,
    pub(crate) gdi_object_count: Option<u64>,
    pub(crate) gpu_percent: Option<f64>,
    pub(crate) gpu_dedicated_bytes: Option<u64>,
    pub(crate) gpu_shared_bytes: Option<u64>,
    pub(crate) dotnet_heap_bytes: Option<u64>,
    pub(crate) io_read_bytes_per_sec: Option<u64>,
    pub(crate) io_write_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkingSetShareSample {
    pub(crate) shareable_bytes: u64,
    pub(crate) shared_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InfoValue {
    Value(String),
    Missing,
    AccessDenied,
    Exited,
    NotAvailable,
    FileMissing,
}

impl InfoValue {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Value(value) => value,
            Self::Missing => "--",
            Self::AccessDenied => "<access denied>",
            Self::Exited => "<exited>",
            Self::NotAvailable => "<not available>",
            Self::FileMissing => "<missing>",
        }
    }

    pub(crate) fn from_option(value: Option<String>) -> Self {
        value
            .filter(|value| !value.trim().is_empty())
            .map(Self::Value)
            .unwrap_or(Self::Missing)
    }
}

impl Default for InfoValue {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessInfo {
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) start_time: Option<u64>,
    pub(crate) ppid: InfoValue,
    pub(crate) parent_process: InfoValue,
    pub(crate) arch: InfoValue,
    pub(crate) user: InfoValue,
    pub(crate) executable: InfoValue,
    pub(crate) command_line: InfoValue,
    pub(crate) file_modified: InfoValue,
    pub(crate) file_size: InfoValue,
    pub(crate) product_version: InfoValue,
    pub(crate) workset_bytes: InfoValue,
    pub(crate) workset_private_bytes: InfoValue,
    pub(crate) ws_shareable_bytes: InfoValue,
    pub(crate) ws_shared_bytes: InfoValue,
}
