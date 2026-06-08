//! Infrastructure adapters: connection pools and outbound clients.
//!
//! Depends on `notegate-core` + `notegate-model` + sqlx/reqwest. No transport
//! framework and no business logic.

pub mod postgres_pool;

pub use postgres_pool::connect;
