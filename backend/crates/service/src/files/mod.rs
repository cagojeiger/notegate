//! File-tree feature: command inputs, output views, validation, the role gate,
//! the patch engine, and the [`FilesService`] over a [`FilesStore`].
//!
//! Command semantics follow `docs/spec/files-commands.md`. The service is pure
//! logic plus the store/authorization trait; the `db` crate implements
//! [`FilesStore`]. Paths are derived from parent links — never stored.

pub mod content;
pub mod input;
pub mod output;
pub mod patch;
pub mod policy;
pub mod service;
pub mod store;
pub mod target;
pub mod validation;

pub use content::{Metrics, compute as content_metrics};
pub use input::{
    ChildrenRequest, CreateDocument, CreateFolder, DeleteNode, Edit, MoveNode, PatchDocument,
    ReadDocument, WriteDocument, WriteTarget,
};
pub use output::{
    ChildrenCursor, ChildrenPage, DeleteResult, DocumentView, NodeView, PatchResult, ReadContent,
    ReadResult,
};
pub use patch::{PatchError, apply_edits};
pub use policy::{FileCommand, require as require_role};
pub use service::FilesService;
pub use store::{FilesStore, StoredContent};
pub use target::{Target, parse_target};
pub use validation::FilesValidationError;
