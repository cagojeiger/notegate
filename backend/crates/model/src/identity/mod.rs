//! Authenticated-caller types shared across the service and api layers.

mod caller;

pub use caller::{Caller, CallerIdentity, Channel, ResolveAttrs};
