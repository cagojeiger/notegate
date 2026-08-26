//! Protocol-neutral NoteGate command contracts.
//!
//! MCP, the HTTP Command API, and the CLI share these request and recovery
//! types so their public JSON stays aligned.

mod input;
mod purpose;
mod recovery;
mod runtime;
mod sequence;
mod tool;

pub use input::{
    CompletedPartInput, FILE_UPLOAD_OP_ABORT_UPLOAD, FILE_UPLOAD_OP_BEGIN_UPLOAD,
    FILE_UPLOAD_OP_COMPLETE_UPLOAD, FILE_UPLOAD_OP_PREPARE_PARTS, FILE_UPLOAD_OPERATIONS,
    FileDownloadInput, FileUploadInput, LINE_EDIT_OP_DELETE_LINES, LINE_EDIT_OP_INSERT_AFTER_LINE,
    LINE_EDIT_OP_INSERT_BEFORE_LINE, LINE_EDIT_OP_REPLACE_LINES, LINE_EDIT_OPERATIONS,
    LineEditInput, MANAGE_OP_CP, MANAGE_OP_MKDIR, MANAGE_OP_MV, MANAGE_OP_RM, MANAGE_OPERATIONS,
    ManageInput, ManageOperationSchema, PatchEdit, READ_OP_CHANGES, READ_OP_LS, READ_OP_READ,
    READ_OP_SPACES, READ_OP_STAT, READ_OP_TREE, READ_OPERATIONS, ReadInput, ReadOperationSchema,
    SEARCH_OP_FIND, SEARCH_OP_GREP, SEARCH_OPERATIONS, SearchInput, SearchOperationSchema,
    WRITE_OP_APPEND, WRITE_OP_EDIT, WRITE_OP_PATCH, WRITE_OP_WRITE, WRITE_OPERATIONS,
    WriteEditEntrySchema, WriteInput, WriteOperationSchema,
};
pub use purpose::{PURPOSE_MAX_CHARS, PurposeValidationError, validate_purpose};
pub use recovery::{RecoveryAction, RecoveryErrorData, RequiredField, ToolCallSpec, ToolCallStep};
pub use runtime::{CommandError, CommandErrorClass};
pub use sequence::{
    RunReadSequenceInput, RunWriteSequenceInput, SEQUENCE_MAX_COMMANDS, SequenceCommand,
    SequenceKind,
};
pub use tool::CommandTool;
