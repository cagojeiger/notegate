//! File-tree persistence internals: row types, error mapping, read queries, and
//! mutating commands. Query modules implement [`crate::FilesRepo`] reads; mutation
//! helpers retain their transaction boundaries. Revision retention is public for reconciliation.

pub(crate) mod commands;
pub(crate) mod error;
pub(crate) mod object_uploads;
pub(crate) mod queries;
pub(crate) mod rows;

pub mod revisions;
