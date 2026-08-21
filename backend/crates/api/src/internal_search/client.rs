use std::time::Duration;

use notegate_search::{
    FindRequest, GrepRequest, SearchCapacity, SearchError, SearchRunError, SearchRuntime,
};
use reqwest::header::CONTENT_TYPE;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use super::auth::{
    InternalSearchAuth, REQUEST_SIGNATURE_HEADER, RESPONSE_SIGNATURE_HEADER, TIMESTAMP_HEADER,
};
use super::contract::{
    ErrorOutput, FindCommand, FindOutput, GrepCommand, GrepOutput, InternalSearchError,
};
use super::{FIND_PATH, GREP_PATH};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SearchClientError {
    #[error(transparent)]
    Search(#[from] SearchError),
    #[error("{0:?} search capacity is busy")]
    Capacity(SearchCapacity),
    #[error("internal search service is unavailable")]
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct SearchClient {
    transport: SearchTransport,
}

#[derive(Clone)]
enum SearchTransport {
    Local(SearchRuntime),
    Http(InternalSearchHttpClient),
    Disabled,
}

impl SearchClient {
    pub(crate) const fn local(runtime: SearchRuntime) -> Self {
        Self {
            transport: SearchTransport::Local(runtime),
        }
    }

    pub(crate) fn http(base_url: &str, signing_key: [u8; 32]) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(
                notegate_core::limits::HTTP_REQUEST_TIMEOUT_SECS,
            ))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            transport: SearchTransport::Http(InternalSearchHttpClient {
                http,
                base_url: base_url.trim_end_matches('/').to_owned(),
                auth: InternalSearchAuth::new(signing_key),
            }),
        })
    }

    pub(crate) const fn disabled() -> Self {
        Self {
            transport: SearchTransport::Disabled,
        }
    }

    pub(crate) async fn find(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: FindRequest,
    ) -> Result<FindOutput, SearchClientError> {
        match &self.transport {
            SearchTransport::Local(runtime) => runtime
                .find(caller_account_id, space_id, request)
                .await
                .map(FindOutput::from)
                .map_err(map_run_error),
            SearchTransport::Http(client) => {
                client
                    .send(
                        FIND_PATH,
                        &FindCommand::new(caller_account_id, space_id, request),
                    )
                    .await
            }
            SearchTransport::Disabled => Err(SearchClientError::Unavailable),
        }
    }

    pub(crate) async fn grep(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: GrepRequest,
    ) -> Result<GrepOutput, SearchClientError> {
        match &self.transport {
            SearchTransport::Local(runtime) => runtime
                .grep(caller_account_id, space_id, request)
                .await
                .map(GrepOutput::from)
                .map_err(map_run_error),
            SearchTransport::Http(client) => {
                client
                    .send(
                        GREP_PATH,
                        &GrepCommand::new(caller_account_id, space_id, request),
                    )
                    .await
            }
            SearchTransport::Disabled => Err(SearchClientError::Unavailable),
        }
    }
}

fn map_run_error(error: SearchRunError) -> SearchClientError {
    match error {
        SearchRunError::Capacity(capacity) => SearchClientError::Capacity(capacity),
        SearchRunError::Search(error) => SearchClientError::Search(error),
    }
}

#[derive(Clone)]
struct InternalSearchHttpClient {
    http: reqwest::Client,
    base_url: String,
    auth: InternalSearchAuth,
}

impl InternalSearchHttpClient {
    async fn send<I, O>(&self, path: &str, input: &I) -> Result<O, SearchClientError>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let body = serde_json::to_vec(input).map_err(|_error| SearchClientError::Unavailable)?;
        let timestamp =
            InternalSearchAuth::now_timestamp().map_err(|_error| SearchClientError::Unavailable)?;
        let signature = self
            .auth
            .sign_request(timestamp, "POST", path, &body)
            .map_err(|_error| SearchClientError::Unavailable)?;
        let mut response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .header(CONTENT_TYPE, "application/json")
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(REQUEST_SIGNATURE_HEADER, signature)
            .body(body)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(event = "internal_search.request_failed", %error);
                SearchClientError::Unavailable
            })?;
        let status = response.status();
        let response_timestamp = response
            .headers()
            .get(TIMESTAMP_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value == timestamp)
            .ok_or(SearchClientError::Unavailable)?;
        let response_signature = response
            .headers()
            .get(RESPONSE_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or(SearchClientError::Unavailable)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(SearchClientError::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_error| SearchClientError::Unavailable)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(SearchClientError::Unavailable);
            }
            body.extend_from_slice(&chunk);
        }
        if !self.auth.verify_response(
            response_timestamp,
            status.as_u16(),
            path,
            &body,
            &response_signature,
        ) {
            return Err(SearchClientError::Unavailable);
        }

        if status.is_success() {
            serde_json::from_slice(&body).map_err(|_error| SearchClientError::Unavailable)
        } else {
            let output: ErrorOutput =
                serde_json::from_slice(&body).map_err(|_error| SearchClientError::Unavailable)?;
            Err(map_wire_error(output.error))
        }
    }
}

fn map_wire_error(error: InternalSearchError) -> SearchClientError {
    let error = match error {
        InternalSearchError::NotFound { message } => SearchError::NotFound(message),
        InternalSearchError::InvalidInput { message } => SearchError::InvalidInput(message),
        InternalSearchError::Forbidden { message } => SearchError::Forbidden(message),
        InternalSearchError::Conflict { message } => SearchError::Conflict(message),
        InternalSearchError::WriteLocked { scope } => SearchError::WriteLocked {
            scope: scope.into(),
        },
        InternalSearchError::UsageRecalculationInProgress {
            retry_after_seconds,
        } => SearchError::UsageRecalculationInProgress {
            retry_after_seconds,
        },
        InternalSearchError::Busy { operation } => {
            return SearchClientError::Capacity(operation.into());
        }
        InternalSearchError::Internal => {
            SearchError::Internal("internal search service error".to_owned())
        }
    };
    SearchClientError::Search(error)
}
