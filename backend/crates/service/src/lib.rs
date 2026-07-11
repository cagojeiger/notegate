//! Business logic for notegate, split per feature like opsgate.
//!
//! Depends on `notegate-core` and `notegate-model` only. It must NOT depend on
//! any transport (axum/rmcp). Services use concrete Postgres repositories from
//! the `db` crate; transport mapping stays in `api`.

pub mod accounts;
pub mod agents;
pub mod api_keys;
pub mod audit_events;
pub mod connections;
pub mod cursor;
pub mod error;
pub mod files;
pub mod identity;
mod pagination;
pub mod search;
pub mod spaces;
pub mod usage;

pub use error::{ServiceError, ServiceResult};
