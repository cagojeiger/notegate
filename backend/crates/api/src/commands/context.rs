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
    source: &'static str,
    invocation_id: Option<uuid::Uuid>,
    edit_session_id: Option<uuid::Uuid>,
    write_purpose: Option<String>,
    internal_search: Option<RequestContext>,
}

impl CommandContext {
    pub fn new(caller: Caller, internal_search: Option<RequestContext>) -> Self {
        let source = match caller.channel {
            notegate_model::Channel::Browser => "browser",
            notegate_model::Channel::Api => "api",
            notegate_model::Channel::Mcp => "mcp",
        };
        Self {
            source,
            invocation_id: None,
            caller,
            edit_session_id: None,
            write_purpose: None,
            internal_search,
        }
    }

    pub fn with_source(mut self, source: &'static str) -> Self {
        self.source = source;
        self
    }

    pub fn with_invocation(mut self, id: uuid::Uuid) -> Self {
        self.invocation_id = Some(id);
        self
    }

    pub fn invocation_id(&self) -> Option<uuid::Uuid> {
        self.invocation_id
    }

    pub fn files(
        &self,
        files: &notegate_service::files::FilesService,
    ) -> notegate_service::files::FilesService {
        files
            .for_channel(self.caller.channel)
            .with_revision_session(self.edit_session_id)
            .with_revision_purpose(self.write_purpose.clone())
            .with_history_source(self.source)
            .with_invocation_id(self.invocation_id)
    }

    pub fn with_edit_session(mut self, session: Option<uuid::Uuid>) -> Self {
        self.edit_session_id = session;
        self
    }

    pub fn with_write_purpose(mut self, purpose: String) -> Self {
        self.write_purpose = Some(purpose);
        self
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn internal_search(&self) -> Option<&RequestContext> {
        self.internal_search.as_ref()
    }
}
