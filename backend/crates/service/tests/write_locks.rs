//! Integration coverage for the node write-lock contract.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;
#[path = "write_locks/management.rs"]
mod management;
#[path = "write_locks/mutations.rs"]
mod mutations;
#[path = "write_locks/projection.rs"]
mod projection;
#[path = "write_locks/uploads.rs"]
mod uploads;
#[path = "common/write_lock.rs"]
mod write_lock_support;
