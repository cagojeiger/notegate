use axum::http::{HeaderMap, HeaderValue};

pub(crate) const REQUEST_ID_HEADER: &str = "x-request-id";

/// Request-scoped correlation fields propagated across the internal search boundary.
///
/// Keeping the carrier separate from search commands lets W3C trace context be added
/// without changing the search domain contract.
#[derive(Debug, Clone, Default)]
pub(crate) struct RequestContext {
    request_id: Option<HeaderValue>,
}

impl RequestContext {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            request_id: headers.get(REQUEST_ID_HEADER).cloned(),
        }
    }

    pub(crate) fn request_id(&self) -> Option<&HeaderValue> {
        self.request_id.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_copies_only_supported_correlation_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("request-123"));
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));

        let context = RequestContext::from_headers(&headers);

        assert_eq!(
            context.request_id().and_then(|value| value.to_str().ok()),
            Some("request-123")
        );
    }
}
