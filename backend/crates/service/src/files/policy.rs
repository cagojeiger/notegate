//! Role gate for file-tree commands.
//!
//! Authorization is per workspace (`docs/spec/db.md`):
//!
//! ```text
//! viewer = list/stat/read/find/grep
//! editor = viewer + write/patch/mkdir/touch/move/delete (+ restore)
//! owner  = editor
//! ```
//!
//! Workspace access management is gated in `AccessService`, not in this file-tree
//! policy. The file service resolves the caller's live [`Role`] first (no role ⇒
//! `404`), then
//! calls [`require`] before doing any work. A role below the command's minimum is
//! reported as forbidden (`403`).

use notegate_model::Role;

use crate::error::{ServiceError, ServiceResult};

/// A file-tree command, used to gate by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCommand {
    /// List a folder's direct children.
    Ls,
    /// Read node metadata.
    Stat,
    /// Read document content.
    Read,
    /// Search node metadata by name.
    Find,
    /// Search document content.
    Grep,
    /// Create a folder.
    Mkdir,
    /// Create an empty document.
    Touch,
    /// Replace document content.
    Write,
    /// Apply exact targeted edits to a document.
    Patch,
    /// Move or rename a node.
    Mv,
    /// Soft-delete a node.
    Rm,
    /// Restore a soft-deleted node.
    Restore,
}

impl FileCommand {
    /// The minimum role required to run this command.
    pub fn min_role(self) -> Role {
        match self {
            Self::Ls | Self::Stat | Self::Read | Self::Find | Self::Grep => Role::Viewer,
            Self::Mkdir
            | Self::Touch
            | Self::Write
            | Self::Patch
            | Self::Mv
            | Self::Rm
            | Self::Restore => Role::Editor,
        }
    }

    /// A stable label for error messages.
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
            Self::Patch => "patch",
            Self::Mv => "mv",
            Self::Rm => "rm",
            Self::Restore => "restore",
        }
    }
}

/// Require `role` to be sufficient for `command`, else forbidden (`403`).
///
/// The caller must already have mapped "no live role" to not-found (`404`); this
/// only compares a present role against the command minimum.
pub fn require(role: Role, command: FileCommand) -> ServiceResult<()> {
    if role < command.min_role() {
        return Err(ServiceError::Forbidden(format!(
            "{} requires at least {} role",
            command.label(),
            command.min_role().as_str()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use super::*;

    /// The full policy table from the spec: which roles may run which commands.
    fn allowed(role: Role, command: FileCommand) -> bool {
        require(role, command).is_ok()
    }

    #[test]
    fn viewer_can_only_read() {
        let read = [
            FileCommand::Ls,
            FileCommand::Stat,
            FileCommand::Read,
            FileCommand::Find,
            FileCommand::Grep,
        ];
        for command in read {
            assert!(
                allowed(Role::Viewer, command),
                "viewer should allow {command:?}"
            );
        }
        let mutate = [
            FileCommand::Mkdir,
            FileCommand::Touch,
            FileCommand::Write,
            FileCommand::Patch,
            FileCommand::Mv,
            FileCommand::Rm,
            FileCommand::Restore,
        ];
        for command in mutate {
            assert!(
                !allowed(Role::Viewer, command),
                "viewer should deny {command:?}"
            );
        }
    }

    #[test]
    fn editor_can_mutate() {
        let mutate = [
            FileCommand::Ls,
            FileCommand::Read,
            FileCommand::Mkdir,
            FileCommand::Touch,
            FileCommand::Write,
            FileCommand::Patch,
            FileCommand::Mv,
            FileCommand::Rm,
            FileCommand::Restore,
        ];
        for command in mutate {
            assert!(
                allowed(Role::Editor, command),
                "editor should allow {command:?}"
            );
        }
    }

    #[test]
    fn owner_can_do_everything() {
        let all = [
            FileCommand::Ls,
            FileCommand::Stat,
            FileCommand::Read,
            FileCommand::Find,
            FileCommand::Grep,
            FileCommand::Mkdir,
            FileCommand::Touch,
            FileCommand::Write,
            FileCommand::Patch,
            FileCommand::Mv,
            FileCommand::Rm,
            FileCommand::Restore,
        ];
        for command in all {
            assert!(
                allowed(Role::Owner, command),
                "owner should allow {command:?}"
            );
        }
    }

    #[test]
    fn insufficient_role_is_forbidden() {
        let err = require(Role::Viewer, FileCommand::Write).unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden(_)));
    }
}
