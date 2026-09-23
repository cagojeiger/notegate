//! Pure text processing: logical lines, content metrics, edits, and syntax checks.
//!
//! This crate has no dependency on NoteGate services, storage, or transports.
//! Callers own authorization, size limits, persistence, and error mapping.

pub mod content;
pub mod format;
pub mod lines;
pub mod patch;

pub use patch::{Edit, LineEdit, PatchMode};
