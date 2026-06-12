//! Permission gate for file-tree commands.
//!
//! `read` permission allows list/stat/read/find/grep. `write` permission also
//! allows mutation commands.

use notegate_model::Permission;

use crate::error::{ServiceError, ServiceResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCommand {
    Ls,
    Stat,
    Read,
    Find,
    Grep,
    Mkdir,
    Touch,
    Write,
    Append,
    Patch,
    Edit,
    Copy,
    Mv,
    Rm,
}

impl FileCommand {
    pub fn requires_write(self) -> bool {
        matches!(
            self,
            Self::Mkdir
                | Self::Touch
                | Self::Write
                | Self::Append
                | Self::Patch
                | Self::Edit
                | Self::Copy
                | Self::Mv
                | Self::Rm
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ls => "ls",
            Self::Stat => "stat",
            Self::Read => "read",
            Self::Find => "find",
            Self::Grep => "grep",
            Self::Mkdir => "mkdir",
            Self::Touch => "touch",
            Self::Write => "write",
            Self::Append => "append",
            Self::Patch => "patch",
            Self::Edit => "edit",
            Self::Copy => "copy",
            Self::Mv => "mv",
            Self::Rm => "rm",
        }
    }
}

pub fn require(permission: Permission, command: FileCommand) -> ServiceResult<()> {
    if command.requires_write() && !permission.allows_write() {
        return Err(ServiceError::Forbidden(format!(
            "{} requires write permission",
            command.label()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn allowed(permission: Permission, command: FileCommand) -> bool {
        require(permission, command).is_ok()
    }

    #[test]
    fn read_permission_cannot_mutate() {
        assert!(allowed(Permission::Read, FileCommand::Ls));
        assert!(allowed(Permission::Read, FileCommand::Read));
        assert!(allowed(Permission::Read, FileCommand::Find));
        assert!(!allowed(Permission::Read, FileCommand::Write));
        assert!(!allowed(Permission::Read, FileCommand::Append));
        assert!(!allowed(Permission::Read, FileCommand::Rm));
    }

    #[test]
    fn write_permission_can_mutate() {
        assert!(allowed(Permission::Write, FileCommand::Ls));
        assert!(allowed(Permission::Write, FileCommand::Write));
        assert!(allowed(Permission::Write, FileCommand::Append));
        assert!(allowed(Permission::Write, FileCommand::Rm));
    }
}
