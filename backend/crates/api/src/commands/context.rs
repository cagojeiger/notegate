//! Authenticated, request-scoped state shared by command handlers.

use notegate_model::{Caller, files::FileMutationContext};

use crate::internal_search::RequestContext;

/// Transport-neutral context established by an authenticated adapter.
///
/// The caller is always available. Internal-search metadata is optional because
/// only search commands need an ingress deadline and correlation id.
#[derive(Debug, Clone)]
pub struct CommandContext {
    caller: Caller,
    mutation: FileMutationContext,
    internal_search: Option<RequestContext>,
}

impl CommandContext {
    pub fn new(caller: Caller, internal_search: Option<RequestContext>) -> Self {
        Self {
            mutation: FileMutationContext::for_channel(caller.channel),
            caller,
            internal_search,
        }
    }

    pub fn with_source(mut self, source: &'static str) -> Self {
        self.mutation.source = source;
        self
    }

    pub fn with_invocation(mut self, id: uuid::Uuid) -> Self {
        self.mutation.invocation_id = Some(id);
        self
    }

    pub fn invocation_id(&self) -> Option<uuid::Uuid> {
        self.mutation.invocation_id
    }

    pub fn files(
        &self,
        files: &notegate_service::files::FilesService,
    ) -> notegate_service::files::FilesService {
        files
            .for_channel(self.caller.channel)
            .with_mutation_context(self.mutation.clone())
    }

    pub fn with_edit_session(mut self, session: Option<uuid::Uuid>) -> Self {
        self.mutation.edit_session_id = session;
        self
    }

    pub fn with_write_purpose(mut self, purpose: String) -> Self {
        self.mutation.purpose = Some(purpose);
        self
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn internal_search(&self) -> Option<&RequestContext> {
        self.internal_search.as_ref()
    }
}
