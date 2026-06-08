//! File-tree persistence internals: row types, error mapping, read queries, and
//! mutating commands. The public [`crate::FilesRepo`] composes these to implement
//! the `notegate-service` `FilesStore` and `SearchStore` traits.

pub mod commands;
pub mod error;
pub mod queries;
pub mod rows;
