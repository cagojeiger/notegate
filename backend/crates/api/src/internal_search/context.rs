use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, request::Parts};

use notegate_core::limits;

pub(crate) const REQUEST_ID_HEADER: &str = "x-request-id";
pub(super) const SEARCH_RESPONSE_HEADROOM: Duration = Duration::from_secs(1);

/// Absolute request budget established by the public data-plane boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestDeadline {
    instant: Instant,
    unix_ms: i64,
}

impl RequestDeadline {
    pub(crate) fn after(duration: Duration) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        let duration_ms = duration.as_millis();
        Self {
            instant: Instant::now() + duration,
            unix_ms: i64::try_from(now_ms.saturating_add(duration_ms)).unwrap_or(i64::MAX),
        }
    }

    fn remaining(self) -> Option<Duration> {
        self.instant
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
    }

    fn search_deadline_unix_ms(self) -> Option<i64> {
        self.remaining()?.checked_sub(SEARCH_RESPONSE_HEADROOM)?;
        Some(
            self.unix_ms
                .saturating_sub(i64::try_from(SEARCH_RESPONSE_HEADROOM.as_millis()).ok()?),
        )
    }
}

/// Request-scoped metadata propagated across the internal search boundary.
///
/// Keeping the carrier separate from search commands lets W3C trace context be added
/// without changing the search domain contract.
#[derive(Debug, Clone)]
pub(crate) struct RequestContext {
    request_id: Option<HeaderValue>,
    deadline: RequestDeadline,
}

impl RequestContext {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            request_id: headers.get(REQUEST_ID_HEADER).cloned(),
            deadline: RequestDeadline::after(Duration::from_secs(
                limits::HTTP_REQUEST_TIMEOUT_SECS,
            )),
        }
    }

    pub(crate) fn from_parts(parts: &Parts) -> Self {
        Self {
            request_id: parts.headers.get(REQUEST_ID_HEADER).cloned(),
            deadline: parts
                .extensions
                .get::<RequestDeadline>()
                .copied()
                .unwrap_or_else(|| {
                    RequestDeadline::after(Duration::from_secs(limits::HTTP_REQUEST_TIMEOUT_SECS))
                }),
        }
    }

    pub(crate) fn request_id(&self) -> Option<&HeaderValue> {
        self.request_id.as_ref()
    }

    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.deadline.remaining()
    }

    pub(crate) fn search_deadline_unix_ms(&self) -> Option<i64> {
        self.deadline.search_deadline_unix_ms()
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(duration: Duration) -> Self {
        Self {
            request_id: None,
            deadline: RequestDeadline::after(duration),
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::from_headers(&HeaderMap::new())
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

    #[test]
    fn request_context_uses_the_ingress_deadline_and_reserves_response_time() {
        let mut request = axum::http::Request::new(());
        request
            .extensions_mut()
            .insert(RequestDeadline::after(Duration::from_secs(10)));
        let (parts, _) = request.into_parts();

        let context = RequestContext::from_parts(&parts);

        assert!(
            context
                .remaining()
                .is_some_and(|value| value <= Duration::from_secs(10))
        );
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        let deadline_ms = context.search_deadline_unix_ms().unwrap_or_default();
        assert!(i128::from(deadline_ms) > i128::try_from(now_ms).unwrap_or(i128::MAX));
    }

    #[test]
    fn elapsed_request_has_no_search_budget() {
        let context = RequestContext {
            request_id: None,
            deadline: RequestDeadline::after(Duration::ZERO),
        };

        assert!(context.remaining().is_none());
        assert!(context.search_deadline_unix_ms().is_none());
    }
}
