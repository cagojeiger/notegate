//! Authenticated, request-scoped state shared by command handlers.

use notegate_model::Caller;

use crate::internal_search::RequestContext;

/// Transport-neutral context established by an authenticated adapter.
///
/// The caller is always available. Internal-search metadata is optional because
/// only search commands need an ingress deadline and correlation id.
#[derive(Debug, Clone)]
pub struct CommandContext {
    caller: Caller,
    internal_search: Option<RequestContext>,
}

impl CommandContext {
    pub fn new(caller: Caller, internal_search: Option<RequestContext>) -> Self {
        Self {
            caller,
            internal_search,
        }
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn internal_search(&self) -> Option<&RequestContext> {
        self.internal_search.as_ref()
    }
}
