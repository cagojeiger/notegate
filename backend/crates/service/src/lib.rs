//! Business logic for notegate, split per feature like opsgate.
//!
//! Depends on `notegate-core`, `notegate-model`, and concrete Postgres repositories
//! from `notegate-db`. It must NOT depend on any transport (axum/rmcp); transport
//! mapping stays in `api`.

pub mod accounts;
pub mod agents;
pub mod api_keys;
pub mod audit_events;
pub mod background_jobs;
pub mod command_invocations;
pub mod connections;
pub mod cursor;
pub mod error;
pub mod files;
pub mod identity;
pub mod link_graph;
mod pagination;
pub mod spaces;
pub mod usage;

pub use error::{ServiceError, ServiceResult};
