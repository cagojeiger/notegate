use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use notegate_command::CommandError;
use notegate_model::Caller;
use serde_json::json;

use super::error::CommandHttpError;
use crate::commands::CommandContext;
use crate::internal_search::RequestContext;

/// Authenticated, request-scoped command context extracted at the HTTP edge.
#[derive(Debug)]
pub(super) struct HttpCommandContext(CommandContext);

impl HttpCommandContext {
    pub(super) fn as_command(&self) -> &CommandContext {
        &self.0
    }
}

impl<S> FromRequestParts<S> for HttpCommandContext
where
    S: Send + Sync,
{
    type Rejection = CommandHttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let caller = parts.extensions.get::<Caller>().cloned().ok_or_else(|| {
            tracing::error!(event = "command_api.context.caller_missing");
            CommandError::internal("authenticated request context is unavailable").with_data(
                json!({
                    "kind": "internal_error",
                    "code": "internal_error",
                }),
            )
        })?;

        Ok(Self(CommandContext::new(
            caller,
            RequestContext::from_parts(parts),
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::time::Duration;

    use axum::http::Request;
    use chrono::Utc;
    use notegate_model::account::{Account, AccountKind};
    use notegate_model::agent::Agent;
    use notegate_model::{CallerIdentity, Channel};
    use uuid::Uuid;

    use super::*;
    use crate::internal_search::RequestDeadline;

    fn agent_caller() -> Caller {
        let id = Uuid::new_v4();
        Caller {
            account: Account {
                id,
                kind: AccountKind::Agent,
                display_name: "command-api-test".to_owned(),
                is_active: true,
                deleted_at: None,
                deleted_by: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            identity: CallerIdentity::Agent(Agent {
                id,
                name: "command-api-test".to_owned(),
                owner_user_id: Uuid::new_v4(),
            }),
            channel: Channel::Api,
        }
    }

    #[tokio::test]
    async fn missing_authenticated_context_is_a_stable_internal_error() {
        let (mut parts, _) = Request::new(()).into_parts();

        let error = HttpCommandContext::from_request_parts(&mut parts, &())
            .await
            .expect_err("caller extension is required");

        assert_eq!(
            error.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(error.body().error, "internal_error");
        assert_eq!(error.body().kind, "internal_error");
    }

    #[tokio::test]
    async fn authenticated_context_propagates_deadline_and_request_id() {
        let caller = agent_caller();
        let account_id = caller.account_id();
        let mut request = Request::new(());
        request.headers_mut().insert(
            "x-request-id",
            axum::http::HeaderValue::from_static("command-request-123"),
        );
        request.extensions_mut().insert(caller);
        request
            .extensions_mut()
            .insert(RequestDeadline::after(Duration::from_secs(10)));
        let (mut parts, _) = request.into_parts();

        let context = HttpCommandContext::from_request_parts(&mut parts, &())
            .await
            .expect("authenticated command context");

        assert_eq!(context.as_command().caller().account_id(), account_id);
        assert_eq!(
            context
                .as_command()
                .internal_search()
                .and_then(|value| value.request_id())
                .and_then(|value| value.to_str().ok()),
            Some("command-request-123")
        );
        assert!(
            context
                .as_command()
                .internal_search()
                .and_then(|value| value.remaining())
                .is_some()
        );
    }
}
