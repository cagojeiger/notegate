#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KnownMcpTool {
    Me,
    Read,
    Search,
    Write,
    Manage,
    FileDownload,
    FileUpload,
    RunReadSequence,
    RunWriteSequence,
}

impl KnownMcpTool {
    #[cfg(test)]
    pub(super) const ALL: [Self; 9] = [
        Self::Me,
        Self::Read,
        Self::Search,
        Self::Write,
        Self::Manage,
        Self::FileDownload,
        Self::FileUpload,
        Self::RunReadSequence,
        Self::RunWriteSequence,
    ];

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "me" => Some(Self::Me),
            "read" => Some(Self::Read),
            "search" => Some(Self::Search),
            "write" => Some(Self::Write),
            "manage" => Some(Self::Manage),
            "file_download" => Some(Self::FileDownload),
            "file_upload" => Some(Self::FileUpload),
            "run_read_sequence" => Some(Self::RunReadSequence),
            "run_write_sequence" => Some(Self::RunWriteSequence),
            _ => None,
        }
    }

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Read => "read",
            Self::Search => "search",
            Self::Write => "write",
            Self::Manage => "manage",
            Self::FileDownload => "file_download",
            Self::FileUpload => "file_upload",
            Self::RunReadSequence => "run_read_sequence",
            Self::RunWriteSequence => "run_write_sequence",
        }
    }

    pub(super) const fn accepts_op(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Search | Self::Write | Self::Manage | Self::FileUpload
        )
    }

    pub(super) const fn is_sequence(self) -> bool {
        matches!(self, Self::RunReadSequence | Self::RunWriteSequence)
    }

    pub(super) const fn is_sequence_command(self) -> bool {
        matches!(self, Self::Read | Self::Search | Self::Write | Self::Manage)
    }
}
