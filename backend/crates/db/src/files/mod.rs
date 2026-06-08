//! File-tree persistence internals: row types, error mapping, read queries, and
//! mutating commands. The public [`crate::FilesRepo`] composes these to implement
//! concrete persistence methods consumed by `notegate-service`.

pub mod commands;
pub mod error;
pub mod queries;
pub mod rows;
