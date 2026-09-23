//! Byte-based media detection and bounded DOCX validation.
//! Storage, scheduling, preview policy, and logging belong to the caller.

mod detect;
mod docx;

pub use detect::{
    MediaDetection, detect_media_type_from_bytes, is_docx_candidate, sniff_media_type,
};
pub use docx::DocxRejection;

pub const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";
