//! Business logic for notegate, split per feature like opsgate.
//!
//! Depends on `notegate-core` and `notegate-model` only. It must NOT depend on
//! any transport (axum/rmcp) or storage driver (sqlx) — those edges are guarded
//! in CI. Services use concrete Postgres repositories from the `db` crate.

pub mod access;
pub mod agents;
pub mod cursor;
pub mod error;
pub mod files;
pub mod identity;
mod pagination;
pub mod search;
pub mod workspaces;

pub use error::{ServiceError, ServiceResult};
