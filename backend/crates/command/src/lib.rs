//! Protocol-neutral NoteGate command contracts.
//!
//! Transport adapters such as MCP and the future CLI API share these request
//! and recovery types so their public JSON stays aligned.

mod input;
mod purpose;
mod recovery;
mod runtime;

pub use input::{
    CompletedPartInput, FileDownloadInput, FileUploadInput, LineEditInput, ManageInput,
    ManageOperationSchema, PatchEdit, ReadInput, ReadOperationSchema, SearchInput,
    SearchOperationSchema, WriteEditEntrySchema, WriteInput, WriteOperationSchema,
};
pub use purpose::{PURPOSE_MAX_CHARS, PurposeValidationError, validate_purpose};
pub use recovery::{RecoveryAction, RecoveryErrorData, RequiredField, ToolCallSpec, ToolCallStep};
pub use runtime::{CommandError, CommandErrorClass};
