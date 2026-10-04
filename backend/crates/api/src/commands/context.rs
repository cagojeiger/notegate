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
    edit_session_id: Option<uuid::Uuid>,
    internal_search: Option<RequestContext>,
}

impl CommandContext {
    pub fn new(caller: Caller, internal_search: Option<RequestContext>) -> Self {
        Self {
            caller,
            edit_session_id: None,
            internal_search,
        }
    }

    pub fn with_edit_session(mut self, session: Option<uuid::Uuid>) -> Self {
        self.edit_session_id = session;
        self
    }

    pub fn edit_session_id(&self) -> Option<uuid::Uuid> {
        self.edit_session_id
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn internal_search(&self) -> Option<&RequestContext> {
        self.internal_search.as_ref()
    }
}
