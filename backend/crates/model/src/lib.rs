//! Pure domain types for notegate.
//!
//! This crate carries data only — no HTTP, no sqlx, no service logic. It
//! depends only on `notegate-core`. The `service` crate adds behavior; the
//! `db` crate persists these types; the `api` crate maps requests to them.

pub mod account;
pub mod agent;
pub mod document;
pub mod identity;
pub mod node;
pub mod user;
pub mod workspace;

pub use account::{Account, AccountKind, AccountRef};
pub use agent::{Agent, AgentKey};
pub use document::Document;
pub use identity::{Caller, CallerIdentity, Channel};
pub use node::{Node, NodeKind};
pub use user::User;
pub use workspace::{Role, Workspace, WorkspaceAccess};
