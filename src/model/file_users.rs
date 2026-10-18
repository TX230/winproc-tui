use serde::{Deserialize, Serialize};

use super::{ProcessIdentity, ProcessRow};

pub(crate) const MAX_QUERY_UNITS: usize = 4096;
pub(crate) const MAX_MATCHES: usize = 5000;
pub(crate) const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_SCAN_HANDLES: usize = 500_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FileSearchMode {
    #[default]
    FileName,
    Path,
    ExactPath,
}

impl FileSearchMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::FileName => "Filename contains",
            Self::Path => "Path contains",
            Self::ExactPath => "Exact path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileSearchQuery {
    pub(crate) text: String,
    pub(crate) mode: FileSearchMode,
}

impl FileSearchQuery {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.text.trim().trim_matches('"').is_empty() {
            return Err("Enter a filename or path".into());
        }
        if self.text.encode_utf16().count() > MAX_QUERY_UNITS
            || self.text.chars().any(char::is_control)
        {
            return Err(
                "Query must be at most 4096 UTF-16 units, without control characters".into(),
            );
        }
        let normalized = normalize_search_path(&self.text);
        if self.mode == FileSearchMode::ExactPath
            && !(normalized.starts_with(r"\\") || normalized.as_bytes().get(1..3) == Some(b":\\"))
        {
            return Err("Exact path requires an absolute drive or UNC path".into());
        }
        Ok(())
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        let path = normalize_search_path(path);
        let query = normalize_search_path(&self.text);
        match self.mode {
            FileSearchMode::FileName => path.rsplit('\\').next().unwrap_or(&path).contains(&query),
            FileSearchMode::Path => path.contains(&query),
            FileSearchMode::ExactPath => {
                normalize_exact_path(&path) == normalize_exact_path(&query)
            }
        }
    }
}

pub(crate) fn normalize_search_path(value: &str) -> String {
    let value = value
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_lowercase();
    if let Some(rest) = value.strip_prefix(r"\\?\unc\") {
        format!(r"\\{rest}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
    }
}

fn normalize_exact_path(value: &str) -> String {
    let unc = value.starts_with(r"\\");
    let mut parts = Vec::new();
    let protected = if unc { 2 } else { 1 };
    for part in value
        .split('\\')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." && parts.len() > protected {
            parts.pop();
        } else if part != ".." {
            parts.push(part);
        }
    }
    format!("{}{}", if unc { r"\\" } else { "" }, parts.join("\\"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileUserOwner {
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) executable_path: String,
    pub(crate) creation_time: u64,
}

impl FileUserOwner {
    pub(crate) fn identity(&self) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            name: self.name.clone(),
            start_time: self
                .creation_time
                .checked_sub(116_444_736_000_000_000)
                .map(|t| t / 10_000_000),
        }
    }

    pub(crate) fn process_row(&self) -> ProcessRow {
        ProcessRow {
            pid: self.pid,
            name: self.name.clone(),
            start_time: self.identity().start_time,
            executable_path: Some(self.executable_path.clone()),
            ..ProcessRow::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FileUserMatch {
    pub(crate) path: String,
    pub(crate) owner: FileUserOwner,
    pub(crate) handle_count: usize,
}

impl FileUserMatch {
    pub(crate) fn retained_bytes(&self) -> usize {
        self.path.len() + self.owner.name.len() + self.owner.executable_path.len() + 128
    }
    pub(crate) fn plain_text(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.path, self.owner.pid, self.owner.name, self.handle_count
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FileSearchProgress {
    pub(crate) total_processes: usize,
    pub(crate) inspected_processes: usize,
    pub(crate) denied_processes: usize,
    pub(crate) exited_processes: usize,
    pub(crate) unavailable_processes: usize,
    pub(crate) total_handles: usize,
    pub(crate) inspected_handles: usize,
    pub(crate) unreadable_handles: usize,
    pub(crate) unnamed_disk_handles: usize,
    pub(crate) skipped_handles: usize,
}

impl FileSearchProgress {
    pub(crate) fn incomplete(&self) -> bool {
        self.denied_processes
            + self.exited_processes
            + self.unavailable_processes
            + self.unreadable_handles
            + self.unnamed_disk_handles
            + self.skipped_handles
            > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FileSearchEnd {
    Complete,
    Cancelled,
    TimedOut,
    HandleLimit,
    ResultLimit,
    Failed(String),
}

impl FileSearchEnd {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Complete => "Finished",
            Self::Cancelled => "Cancelled",
            Self::TimedOut => "Timed out",
            Self::HandleLimit => "Handle limit reached",
            Self::ResultLimit => "Result limit reached",
            Self::Failed(_) => "Failed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileSearchReport {
    pub(crate) matches: Vec<FileUserMatch>,
    pub(crate) progress: FileSearchProgress,
    pub(crate) end: Option<FileSearchEnd>,
    pub(crate) cleanup_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum FileUsersRequest {
    Search(FileSearchQuery),
    Verify(FileUserOwner),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum FileUsersMessage {
    Progress(FileSearchProgress),
    Match(FileUserMatch),
    Owner(FileUserOwner),
    End(FileSearchEnd),
}

#[derive(Debug, Clone)]
pub(crate) struct FileUsersUpdate {
    pub(crate) id: u64,
    pub(crate) report: FileSearchReport,
    pub(crate) owner: Option<FileUserOwner>,
}
