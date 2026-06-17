use axum::http::HeaderMap;
use axum::http::header::{ORIGIN, REFERER};

use crate::state::AppState;

pub(crate) fn has_trusted_browser_origin(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .or_else(|| headers.get(REFERER).and_then(|value| value.to_str().ok()))
        .is_some_and(|source| {
            same_origin(source, &state.config.notegate_public_url)
                || same_origin(source, &state.config.resource_url)
        })
}

fn same_origin(source: &str, trusted: &str) -> bool {
    let Ok(source) = url::Url::parse(source) else {
        return false;
    };
    let Ok(trusted) = url::Url::parse(trusted) else {
        return false;
    };

    source.scheme() == trusted.scheme()
        && source.host_str() == trusted.host_str()
        && source.port_or_known_default() == trusted.port_or_known_default()
}
