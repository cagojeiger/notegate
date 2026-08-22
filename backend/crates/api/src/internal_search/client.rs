use std::time::Duration;

use notegate_search::{FindRequest, GrepRequest, SearchCapacity, SearchError};
#[cfg(test)]
use notegate_search::{SearchRunError, SearchRuntime};
use reqwest::header::CONTENT_TYPE;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use super::auth::{
    InternalSearchAuth, REQUEST_SIGNATURE_HEADER, RESPONSE_SIGNATURE_HEADER, TIMESTAMP_HEADER,
};
use super::context::{REQUEST_ID_HEADER, RequestContext};
use super::contract::{
    ErrorOutput, FindCommand, FindOutput, GrepCommand, GrepOutput, InternalSearchError,
    InternalSearchRequest,
};
use super::{FIND_PATH, GREP_PATH};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SearchClientError {
    #[error(transparent)]
    Search(#[from] SearchError),
    #[error("{0:?} search capacity is busy")]
    Capacity(SearchCapacity),
    #[error("search request deadline exceeded")]
    DeadlineExceeded,
    #[error("internal search service is unavailable")]
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct SearchClient {
    transport: SearchTransport,
}

#[derive(Clone)]
enum SearchTransport {
    #[cfg(test)]
    Local(SearchRuntime),
    Http(InternalSearchHttpClient),
    Disabled,
}

impl SearchClient {
    #[cfg(test)]
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
        context: &RequestContext,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: FindRequest,
    ) -> Result<FindOutput, SearchClientError> {
        match &self.transport {
            #[cfg(test)]
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
                        context,
                    )
                    .await
            }
            SearchTransport::Disabled => Err(SearchClientError::Unavailable),
        }
    }

    pub(crate) async fn grep(
        &self,
        context: &RequestContext,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: GrepRequest,
    ) -> Result<GrepOutput, SearchClientError> {
        match &self.transport {
            #[cfg(test)]
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
                        context,
                    )
                    .await
            }
            SearchTransport::Disabled => Err(SearchClientError::Unavailable),
        }
    }
}

#[cfg(test)]
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
    async fn send<I, O>(
        &self,
        path: &str,
        input: &I,
        context: &RequestContext,
    ) -> Result<O, SearchClientError>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let search_timeout = context
            .search_timeout()
            .ok_or(SearchClientError::DeadlineExceeded)?;
        let timeout_ms = u64::try_from(search_timeout.as_millis())
            .ok()
            .filter(|value| *value > 0)
            .ok_or(SearchClientError::DeadlineExceeded)?;
        let body = serde_json::to_vec(&InternalSearchRequest {
            timeout_ms,
            command: input,
        })
        .map_err(|_error| SearchClientError::Unavailable)?;
        let timestamp =
            InternalSearchAuth::now_timestamp().map_err(|_error| SearchClientError::Unavailable)?;
        let signature = self
            .auth
            .sign_request(timestamp, "POST", path, &body)
            .map_err(|_error| SearchClientError::Unavailable)?;
        let mut request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .header(CONTENT_TYPE, "application/json")
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(REQUEST_SIGNATURE_HEADER, signature)
            .body(body);
        if let Some(request_id) = context.request_id() {
            request = request.header(REQUEST_ID_HEADER, request_id.clone());
        }
        let remaining = context
            .remaining()
            .ok_or(SearchClientError::DeadlineExceeded)?;
        let mut response = request
            .timeout(remaining)
            .send()
            .await
            .map_err(map_transport_error)?;
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
        while let Some(chunk) = response.chunk().await.map_err(map_transport_error)? {
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

        if status == reqwest::StatusCode::OK {
            serde_json::from_slice(&body).map_err(|_error| SearchClientError::Unavailable)
        } else if status.is_success() {
            tracing::warn!(
                event = "internal_search.protocol_error",
                %status,
                reason = "unexpected_success_status",
            );
            Err(SearchClientError::Unavailable)
        } else {
            let output: ErrorOutput =
                serde_json::from_slice(&body).map_err(|_error| SearchClientError::Unavailable)?;
            if output.error.status() != status {
                tracing::warn!(
                    event = "internal_search.protocol_error",
                    %status,
                    error_code = output.error.code(),
                    reason = "status_error_mismatch",
                );
                return Err(SearchClientError::Unavailable);
            }
            Err(map_wire_error(output.error))
        }
    }
}

fn map_transport_error(error: reqwest::Error) -> SearchClientError {
    tracing::warn!(event = "internal_search.request_failed", %error);
    if error.is_timeout() {
        SearchClientError::DeadlineExceeded
    } else {
        SearchClientError::Unavailable
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
        InternalSearchError::DeadlineExceeded => return SearchClientError::DeadlineExceeded,
        InternalSearchError::Internal => {
            SearchError::Internal("internal search service error".to_owned())
        }
    };
    SearchClientError::Search(error)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::convert::Infallible;

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, Request, Response, StatusCode};
    use axum::routing::post;
    use futures_util::stream;
    use serde_json::{Value, json};

    use super::*;
    use crate::internal_search::contract::{ErrorOutput, WriteLockScopeWire};

    const SIGNING_KEY: [u8; 32] = [7; 32];

    #[derive(Clone)]
    struct StubResponse {
        status: StatusCode,
        body: Vec<u8>,
        timestamp_offset: Option<i64>,
        signing_key: [u8; 32],
        signed_body: Option<Vec<u8>>,
        stream_body: bool,
        expected_request_id: Option<HeaderValue>,
        expected_command: Option<Value>,
    }

    impl StubResponse {
        fn signed(status: StatusCode, body: Vec<u8>) -> Self {
            Self {
                status,
                body,
                timestamp_offset: Some(0),
                signing_key: SIGNING_KEY,
                signed_body: None,
                stream_body: false,
                expected_request_id: None,
                expected_command: None,
            }
        }
    }

    async fn stub_handler(
        State(response): State<StubResponse>,
        request: Request<Body>,
    ) -> Response<Body> {
        let (parts, body) = request.into_parts();
        if let Some(expected_request_id) = &response.expected_request_id {
            assert_eq!(
                parts.headers.get(REQUEST_ID_HEADER),
                Some(expected_request_id)
            );
        }
        let request_timestamp = parts
            .headers
            .get(TIMESTAMP_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_default();
        let request_signature = parts
            .headers
            .get(REQUEST_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let request_body = to_bytes(body, MAX_RESPONSE_BYTES)
            .await
            .expect("bounded test request body");
        assert!(InternalSearchAuth::new(SIGNING_KEY).verify_request_at(
            request_timestamp,
            request_timestamp,
            "POST",
            FIND_PATH,
            &request_body,
            request_signature,
        ));
        let request_json: Value =
            serde_json::from_slice(&request_body).expect("client sends JSON request envelope");
        assert!(
            request_json
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .is_some_and(|value| value > 0)
        );
        if let Some(expected_command) = &response.expected_command {
            assert_eq!(request_json.get("command"), Some(expected_command));
        }
        let signed_body = response.signed_body.as_deref().unwrap_or(&response.body);
        let signature = InternalSearchAuth::new(response.signing_key)
            .sign_response(
                request_timestamp,
                response.status.as_u16(),
                FIND_PATH,
                signed_body,
            )
            .expect("fixed-size signing key is valid");
        let body = if response.stream_body {
            Body::from_stream(stream::iter([Ok::<_, Infallible>(response.body)]))
        } else {
            Body::from(response.body)
        };
        let mut output = Response::builder()
            .status(response.status)
            .header(RESPONSE_SIGNATURE_HEADER, signature)
            .body(body)
            .expect("stub response is valid");
        if let Some(offset) = response.timestamp_offset {
            let value = (request_timestamp + offset)
                .to_string()
                .parse()
                .expect("numeric timestamp is a valid header value");
            output.headers_mut().insert(TIMESTAMP_HEADER, value);
        }
        output
    }

    async fn start_stub(
        response: StubResponse,
    ) -> Result<
        (
            InternalSearchHttpClient,
            tokio::task::JoinHandle<std::io::Result<()>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let app = Router::new()
            .route(FIND_PATH, post(stub_handler))
            .with_state(response);
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let search_client = SearchClient::http(&base_url, SIGNING_KEY)?;
        match search_client.transport {
            SearchTransport::Http(client) => Ok((client, server)),
            SearchTransport::Local(_) | SearchTransport::Disabled => {
                Err(std::io::Error::other("expected HTTP search transport").into())
            }
        }
    }

    async fn send(response: StubResponse) -> Result<SearchClientError, Box<dyn std::error::Error>> {
        let (client, server) = start_stub(response).await?;
        let result = client
            .send::<_, Value>(FIND_PATH, &json!({}), &RequestContext::default())
            .await;
        server.abort();
        match result {
            Ok(_) => Err(std::io::Error::other("stub response unexpectedly succeeded").into()),
            Err(error) => Ok(error),
        }
    }

    async fn send_error(
        status: StatusCode,
        error: InternalSearchError,
    ) -> Result<SearchClientError, Box<dyn std::error::Error>> {
        let body = serde_json::to_vec(&ErrorOutput { error })?;
        send(StubResponse::signed(status, body)).await
    }

    #[tokio::test]
    async fn signed_errors_preserve_every_wire_error_semantic()
    -> Result<(), Box<dyn std::error::Error>> {
        let error = send_error(
            StatusCode::TOO_MANY_REQUESTS,
            InternalSearchError::busy(SearchCapacity::Grep),
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Capacity(SearchCapacity::Grep)
        ));

        let error = send_error(
            StatusCode::LOCKED,
            InternalSearchError::WriteLocked {
                scope: WriteLockScopeWire::Descendant,
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::WriteLocked {
                scope: notegate_core::WriteLockScope::Descendant
            })
        ));

        let error = send_error(
            StatusCode::BAD_REQUEST,
            InternalSearchError::InvalidInput {
                message: "bad query".to_owned(),
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::InvalidInput(message))
                if message == "bad query"
        ));

        let error = send_error(
            StatusCode::NOT_FOUND,
            InternalSearchError::NotFound {
                message: "missing".to_owned(),
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::NotFound(message))
                if message == "missing"
        ));

        let error = send_error(
            StatusCode::FORBIDDEN,
            InternalSearchError::Forbidden {
                message: "denied".to_owned(),
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::Forbidden(message))
                if message == "denied"
        ));

        let error = send_error(
            StatusCode::CONFLICT,
            InternalSearchError::Conflict {
                message: "stale".to_owned(),
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::Conflict(message))
                if message == "stale"
        ));

        let error = send_error(
            StatusCode::SERVICE_UNAVAILABLE,
            InternalSearchError::UsageRecalculationInProgress {
                retry_after_seconds: 7,
            },
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::UsageRecalculationInProgress {
                retry_after_seconds: 7
            })
        ));

        let error = send_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            InternalSearchError::Internal,
        )
        .await?;
        assert!(matches!(
            error,
            SearchClientError::Search(SearchError::Internal(message))
                if message == "internal search service error"
        ));

        let error = send_error(
            StatusCode::GATEWAY_TIMEOUT,
            InternalSearchError::DeadlineExceeded,
        )
        .await?;
        assert!(matches!(error, SearchClientError::DeadlineExceeded));
        Ok(())
    }

    #[tokio::test]
    async fn elapsed_context_is_rejected_before_transport() -> Result<(), Box<dyn std::error::Error>>
    {
        let (client, server) = start_stub(StubResponse::signed(
            StatusCode::OK,
            br#"{"ok":true}"#.to_vec(),
        ))
        .await?;

        let result = client
            .send::<_, Value>(
                FIND_PATH,
                &json!({}),
                &RequestContext::with_timeout(Duration::ZERO),
            )
            .await;

        server.abort();
        assert!(matches!(result, Err(SearchClientError::DeadlineExceeded)));
        Ok(())
    }

    #[tokio::test]
    async fn contradictory_status_and_error_kind_are_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let error = send_error(
            StatusCode::FORBIDDEN,
            InternalSearchError::InvalidInput {
                message: "bad query".to_owned(),
            },
        )
        .await?;

        assert!(matches!(error, SearchClientError::Unavailable));
        Ok(())
    }

    #[tokio::test]
    async fn success_contract_accepts_only_ok() -> Result<(), Box<dyn std::error::Error>> {
        let response = StubResponse::signed(StatusCode::CREATED, br#"{"ok":true}"#.to_vec());
        assert!(matches!(
            send(response).await?,
            SearchClientError::Unavailable
        ));
        Ok(())
    }

    #[tokio::test]
    async fn request_context_is_forwarded_to_the_search_service()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut response = StubResponse::signed(StatusCode::OK, br#"{"ok":true}"#.to_vec());
        response.expected_request_id = Some(HeaderValue::from_static("request-123"));
        response.expected_command = Some(json!({"q": "needle"}));
        let (client, server) = start_stub(response).await?;
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("request-123"));

        let result = client
            .send::<_, Value>(
                FIND_PATH,
                &json!({"q": "needle"}),
                &RequestContext::from_headers(&headers),
            )
            .await?;

        server.abort();
        assert_eq!(result, json!({ "ok": true }));
        Ok(())
    }

    #[tokio::test]
    async fn response_timestamp_must_be_present_and_echo_the_request()
    -> Result<(), Box<dyn std::error::Error>> {
        for timestamp_offset in [None, Some(1)] {
            let mut response = StubResponse::signed(StatusCode::OK, br#"{"ok":true}"#.to_vec());
            response.timestamp_offset = timestamp_offset;
            assert!(matches!(
                send(response).await?,
                SearchClientError::Unavailable
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn response_signature_binds_the_signing_key_and_body()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut wrong_key = StubResponse::signed(StatusCode::OK, br#"{"ok":true}"#.to_vec());
        wrong_key.signing_key = [9; 32];
        assert!(matches!(
            send(wrong_key).await?,
            SearchClientError::Unavailable
        ));

        let mut tampered_body = StubResponse::signed(StatusCode::OK, br#"{"ok":false}"#.to_vec());
        tampered_body.signed_body = Some(br#"{"ok":true}"#.to_vec());
        assert!(matches!(
            send(tampered_body).await?,
            SearchClientError::Unavailable
        ));
        Ok(())
    }

    #[tokio::test]
    async fn chunked_response_cannot_exceed_the_client_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut response = StubResponse::signed(
            StatusCode::OK,
            vec![b'x'; MAX_RESPONSE_BYTES.saturating_add(1)],
        );
        response.stream_body = true;
        assert!(matches!(
            send(response).await?,
            SearchClientError::Unavailable
        ));
        Ok(())
    }
}
