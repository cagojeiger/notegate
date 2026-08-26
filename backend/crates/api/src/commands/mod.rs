//! Transport-neutral command execution support.
//!
//! HTTP/MCP adapters authenticate a request and build a [`CommandContext`].
//! Command handlers then operate only on that context and return plain JSON so
//! transport-specific envelopes stay at the boundary.

pub(crate) mod context;
pub(crate) mod error;
pub(crate) mod events;
pub(crate) mod executor;
pub(crate) mod files;
pub(crate) mod identity;
pub(crate) mod resolve;
pub(crate) mod search;
pub(crate) mod sequence;
pub(crate) mod spaces;
pub(crate) mod support;
pub(crate) mod transfers;

pub use context::CommandContext;
