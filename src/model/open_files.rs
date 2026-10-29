pub(crate) const FILE_WRITE_THROUGH: u32 = 0x0000_0002;
pub(crate) const FILE_NO_INTERMEDIATE_BUFFERING: u32 = 0x0000_0008;
pub(crate) const FILE_SYNCHRONOUS_IO_ALERT: u32 = 0x0000_0010;
pub(crate) const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileAttributeError {
    NtStatus(i32),
    ShortReply,
    Win32(i32),
}

impl std::fmt::Display for FileAttributeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NtStatus(status) => write!(f, "NTSTATUS 0x{:08X}", *status as u32),
            Self::Win32(code) => write!(f, "Windows error {code}"),
            Self::ShortReply => f.write_str("Incomplete reply"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenFileKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFileMetadata {
    pub(crate) created: Result<u64, FileAttributeError>,
    pub(crate) modified: Result<u64, FileAttributeError>,
    pub(crate) size: Result<Option<u64>, FileAttributeError>,
}

impl Default for OpenFileMetadata {
    fn default() -> Self {
        Self {
            created: Err(FileAttributeError::ShortReply),
            modified: Err(FileAttributeError::ShortReply),
            size: Err(FileAttributeError::ShortReply),
        }
    }
}

pub(crate) fn file_timestamp(value: &Result<u64, FileAttributeError>) -> String {
    match value {
        Ok(0) => "--".into(),
        Ok(ticks) => {
            let seconds = (ticks / 10_000_000) as i64 - 11_644_473_600;
            let nanos = ((ticks % 10_000_000) * 100) as u32;
            chrono::DateTime::from_timestamp(seconds, nanos)
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S%.3f %:z")
                        .to_string()
                })
                .unwrap_or_else(|| "-- (Invalid timestamp)".into())
        }
        Err(error) => format!("-- ({error})"),
    }
}

fn file_size(value: &Result<Option<u64>, FileAttributeError>) -> String {
    match value {
        Ok(Some(bytes)) => {
            let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
            let mut value = *bytes as f64;
            let mut index = 0;
            while value >= 1000.0 && index + 1 < units.len() {
                value /= 1000.0;
                index += 1;
            }
            format!("{value:.2} {} ({bytes} bytes)", units[index])
        }
        Ok(None) => "--".into(),
        Err(error) => format!("-- ({error})"),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFileHandle {
    pub(crate) metadata: OpenFileMetadata,
    pub(crate) value: usize,
    pub(crate) kind: Result<OpenFileKind, FileAttributeError>,
    pub(crate) access: Result<u32, FileAttributeError>,
    pub(crate) mode: Result<u32, FileAttributeError>,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenFileEntry {
    pub(crate) path: String,
    pub(crate) handle: OpenFileHandle,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileAttribute {
    Cache,
    WriteThrough,
    IoMode,
    Access,
}

impl FileAttribute {
    pub(crate) const ALL: [Self; 4] = [Self::Cache, Self::IoMode, Self::Access, Self::WriteThrough];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cache => "Cached",
            Self::WriteThrough => "W-Thru",
            Self::IoMode => "Async",
            Self::Access => "Access",
        }
    }
}

impl OpenFileHandle {
    pub(crate) fn kind_label(&self) -> &'static str {
        match self.kind {
            Ok(OpenFileKind::File) => "F",
            Ok(OpenFileKind::Directory) => "D",
            Err(_) => "--",
        }
    }

    pub(crate) fn attribute(&self, attribute: FileAttribute) -> Option<String> {
        match attribute {
            FileAttribute::Cache => self.mode.as_ref().ok().map(|mode| {
                if mode & FILE_NO_INTERMEDIATE_BUFFERING != 0 {
                    "N"
                } else {
                    "Y"
                }
                .into()
            }),
            FileAttribute::WriteThrough => self.mode.as_ref().ok().map(|mode| {
                if mode & FILE_WRITE_THROUGH != 0 {
                    "Y"
                } else {
                    "N"
                }
                .into()
            }),
            FileAttribute::IoMode => self.mode.as_ref().ok().map(|mode| {
                if mode & (FILE_SYNCHRONOUS_IO_ALERT | FILE_SYNCHRONOUS_IO_NONALERT) != 0 {
                    "N"
                } else {
                    "Y"
                }
                .into()
            }),
            FileAttribute::Access => self.access.as_ref().ok().map(|mask| {
                let names = [(1, "R"), (2, "W"), (4, "A")]
                    .into_iter()
                    .filter_map(|(bit, name)| (mask & bit != 0).then_some(name))
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    "-".into()
                } else {
                    names.concat()
                }
            }),
        }
    }

    pub(crate) fn plain_text(&self, path: &str) -> String {
        let mut fields = vec![
            path.to_string(),
            format!("0x{:X}", self.value),
            self.kind_label().to_string(),
        ];
        fields.extend(
            FileAttribute::ALL.map(|attribute| self.attribute(attribute).unwrap_or("--".into())),
        );
        fields.push(raw_attribute(&self.access));
        fields.push(raw_attribute(&self.mode));
        fields.join("\t")
    }
}

pub(crate) fn raw_attribute(value: &Result<u32, FileAttributeError>) -> String {
    match value {
        Ok(mask) => format!("0x{mask:08X}"),
        Err(error) => format!("-- ({error})"),
    }
}

impl OpenFileEntry {
    pub(crate) fn attribute(&self, attribute: FileAttribute) -> String {
        self.handle.attribute(attribute).unwrap_or("--".into())
    }

    pub(crate) fn has_unknown(&self) -> bool {
        self.handle.kind.is_err() || self.handle.access.is_err() || self.handle.mode.is_err()
    }

    pub(crate) fn detail_text(&self) -> Vec<String> {
        let mut lines = vec![
            self.path.clone(),
            format!("Handle: 0x{:X}", self.handle.value),
            match &self.handle.kind {
                Ok(OpenFileKind::File) => "Type: F (File)".into(),
                Ok(OpenFileKind::Directory) => "Type: D (Directory)".into(),
                Err(error) => format!("Type: -- ({error})"),
            },
        ];
        lines.push(format!(
            "Created: {}",
            file_timestamp(&self.handle.metadata.created)
        ));
        lines.push(format!(
            "Modified: {}",
            file_timestamp(&self.handle.metadata.modified)
        ));
        lines.push(format!("Size: {}", file_size(&self.handle.metadata.size)));
        for attribute in FileAttribute::ALL {
            lines.push(format!(
                "{}: {}",
                attribute.label(),
                self.attribute(attribute)
            ));
        }
        lines.push(format!(
            "Access mask: {}",
            raw_attribute(&self.handle.access)
        ));
        lines.push(format!("Mode flags: {}", raw_attribute(&self.handle.mode)));
        lines
    }

    pub(crate) fn plain_text(&self) -> String {
        self.handle.plain_text(&self.path)
    }
}
