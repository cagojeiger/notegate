//! File command input and output DTOs.

pub use notegate_model::files::{
    AppendText, ChildrenCursor, ChildrenPage, ChildrenRequest, CopyCounts, CopyNode, CopyResult,
    CreateFile, CreateFolder, CreateText, DeleteNode, DeleteResult, Edit, FileContent, FileStats,
    FileView, MoveNode, NodeView, PatchResult, PatchText, ReadContent, ReadResult, ReadText,
    ReadTextBody, StoredContent, StoredFile, TextStats, TextView, WriteTarget, WriteText,
    WriteTextBody,
};
