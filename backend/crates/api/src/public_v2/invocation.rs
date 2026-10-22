//! Authenticated v2 request/response summaries, without buffering either body.

use std::collections::BTreeMap;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Extension, FromRequestParts, MatchedPath, Path, State};
use axum::http::{HeaderMap, Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use notegate_command::{PURPOSE_MAX_CHARS, validate_purpose};
use notegate_model::{Caller, files::FileMutationContext};
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;
use crate::invocations::{InvocationCompletion, InvocationRecord, InvocationSurface};
use crate::observability::CommandInvocationMetrics;
use crate::state::AppState;

pub(crate) const PURPOSE_HEADER: &str = "x-notegate-purpose";

pub(super) async fn capture(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let started = Instant::now();
    let metrics = CommandInvocationMetrics::start(state.config.metrics_enabled, "api", "http");
    let invocation_id = Uuid::new_v4();
    let method = match *request.method() {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        _ => "OTHER",
    };
    // MatchedPath is a server-defined template, never a raw URL or query string.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let operation = format!("{method} {route}");
    let (mut parts, body) = request.into_parts();
    let path_ids: BTreeMap<String, Uuid> =
        Path::<BTreeMap<String, String>>::from_request_parts(&mut parts, &state)
            .await
            .map(|Path(params)| {
                params
                    .into_iter()
                    .filter(|(key, _)| matches!(key.as_str(), "space_id" | "node_id" | "upload_id"))
                    .filter_map(|(key, value)| Uuid::parse_str(&value).ok().map(|id| (key, id)))
                    .collect()
            })
            .unwrap_or_default();
    let (purpose, response) = match request_purpose(&parts.headers) {
        Ok(purpose) => {
            parts.extensions.insert(FileMutationContext {
                invocation_id: Some(invocation_id),
                purpose: purpose.clone(),
                ..FileMutationContext::for_channel(caller.channel)
            });
            (purpose, next.run(Request::from_parts(parts, body)).await)
        }
        Err(error) => (None, error.into_response()),
    };
    let elapsed = started.elapsed();
    let status = response.status();
    let error_code = (status.is_client_error() || status.is_server_error())
        .then(|| format!("http_{}", status.as_u16()));
    let completion = InvocationCompletion::new(metrics, started, elapsed, error_code.as_deref());

    let input =
        json!({"method": method, "route": route, "path_ids": path_ids, "body_recorded": false});
    let snapshot = json!({"status": status.as_u16(), "body_recorded": false});
    completion
        .record(
            &state.command_invocations,
            &caller,
            InvocationRecord {
                id: Some(invocation_id),
                surface: InvocationSurface::Api,
                tool: "http",
                op: Some(&operation),
                purpose: purpose.as_deref(),
                space_name: None,
                input: &input,
                response: Some(&snapshot),
                error_code: error_code.as_deref(),
                elapsed_ms: elapsed.as_millis(),
            },
        )
        .await;
    response
}

/// One percent-encoded UTF-8 value supports non-ASCII reasons without reading bodies.
fn request_purpose(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let mut values = headers.get_all(PURPOSE_HEADER).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(ApiError::invalid_input(
            "X-Notegate-Purpose must occur once",
        ));
    }
    let encoded = value.to_str().map_err(|_| invalid_purpose_encoding())?;
    // A Unicode scalar uses at most four UTF-8 bytes, each encoded as %HH.
    if encoded.len() > PURPOSE_MAX_CHARS * 4 * 3 {
        return Err(ApiError::invalid_input("X-Notegate-Purpose is too long"));
    }
    for (index, byte) in encoded.bytes().enumerate() {
        if byte == b'%'
            && !encoded
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(invalid_purpose_encoding());
        }
    }
    let purpose = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .map_err(|_| invalid_purpose_encoding())?;
    validate_purpose(&purpose).map_err(|error| ApiError::invalid_input(error.to_string()))?;
    if purpose.chars().any(char::is_control) {
        return Err(ApiError::invalid_input(
            "purpose must not contain control characters",
        ));
    }
    Ok(Some(purpose.into_owned()))
}

fn invalid_purpose_encoding() -> ApiError {
    ApiError::invalid_input("X-Notegate-Purpose must be percent-encoded UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn purpose_is_optional_and_decoded_once_without_form_plus_rules()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut headers = HeaderMap::new();
        assert_eq!(request_purpose(&headers).ok(), Some(None));
        for purpose in [
            "review design notes".to_owned(),
            "C++ 100% %20".to_owned(),
            "가".repeat(200),
            "📝".repeat(200),
        ] {
            let encoded =
                percent_encoding::utf8_percent_encode(&purpose, percent_encoding::NON_ALPHANUMERIC)
                    .to_string();
            headers.insert(PURPOSE_HEADER, HeaderValue::from_str(&encoded)?);
            assert_eq!(
                request_purpose(&headers).ok().flatten().as_deref(),
                Some(purpose.as_str())
            );
        }
        headers.insert(PURPOSE_HEADER, HeaderValue::from_static("C++"));
        assert_eq!(
            request_purpose(&headers).ok().flatten().as_deref(),
            Some("C++")
        );
        Ok(())
    }

    #[test]
    fn invalid_duplicate_or_oversized_purpose_is_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut headers = HeaderMap::new();
        for encoded in [
            "",
            "%20reason",
            "reason%20",
            "%",
            "%2",
            "%GG",
            "%FF",
            "x%00y",
            "x%0Ay",
        ] {
            headers.insert(PURPOSE_HEADER, HeaderValue::from_str(encoded)?);
            assert!(request_purpose(&headers).is_err(), "accepted {encoded:?}");
        }
        for value in ["a".repeat(201), "가".repeat(201), "📝".repeat(201)] {
            let encoded =
                percent_encoding::utf8_percent_encode(&value, percent_encoding::NON_ALPHANUMERIC)
                    .to_string();
            headers.insert(PURPOSE_HEADER, HeaderValue::from_str(&encoded)?);
            assert!(request_purpose(&headers).is_err());
        }
        headers.insert(PURPOSE_HEADER, HeaderValue::from_static("valid"));
        headers.append(PURPOSE_HEADER, HeaderValue::from_static("valid"));
        assert!(request_purpose(&headers).is_err());
        Ok(())
    }
}
