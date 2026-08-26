#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTool {
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

impl CommandTool {
    pub const ALL: [Self; 9] = [
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

    pub fn parse(value: &str) -> Option<Self> {
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

    pub const fn as_str(self) -> &'static str {
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

    pub const fn accepts_op(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Search | Self::Write | Self::Manage | Self::FileUpload
        )
    }

    pub const fn is_sequence(self) -> bool {
        matches!(self, Self::RunReadSequence | Self::RunWriteSequence)
    }

    pub const fn is_sequence_command(self) -> bool {
        matches!(self, Self::Read | Self::Search | Self::Write | Self::Manage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_round_trip_through_the_shared_registry() {
        for tool in CommandTool::ALL {
            assert_eq!(CommandTool::parse(tool.as_str()), Some(tool));
        }
    }
}
