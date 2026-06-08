//! MCP tools, one per file. Each module exposes a `call(...)` entry the
//! `#[tool_router]` in [`crate::mcp::server`] dispatches to. Shared plumbing
//! (workspace-name resolution, target parsing, error mapping) lives in
//! [`resolve`]; small shared helpers live in [`support`].

pub mod resolve;
pub mod support;

pub mod me;

pub mod workspaces_create;
pub mod workspaces_get;
pub mod workspaces_list;

pub mod files_find;
pub mod files_grep;
pub mod files_ls;
pub mod files_mkdir;
pub mod files_mv;
pub mod files_patch;
pub mod files_read;
pub mod files_rm;
pub mod files_stat;
pub mod files_touch;
pub mod files_write;
