//! Business logic for notegate, split per feature like opsgate.
//!
//! Depends on `notegate-core`, `notegate-model`, and concrete Postgres repositories
//! from `notegate-db`. It must NOT depend on any transport (axum/rmcp); transport
//! mapping stays in `api`.

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
