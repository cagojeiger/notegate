//! Shared primitives for notegate: configuration, policy, and error types.
//!
//! This crate defines shared contracts but contains no HTTP handlers or database
//! implementation, so every other crate can depend on it without heavy dependencies.

pub mod config;
pub mod error;
pub mod limits;
pub mod security;
pub mod tier;
pub mod validation;

pub use config::{
    BackgroundJobsConfig, Config, HttpRateLimitConfig, HttpRateLimitsConfig, ProcessMode, S3Config,
    SearchBodyCacheConfig,
};
pub use error::{Error, Result, WriteLockScope};
