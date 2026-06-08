//! Shared helpers for MCP tool handlers.

use rmcp::ErrorData;
use serde::Serialize;
use serde_json::{Value, json};

use super::resolve::encode_cursor;

/// Clamp a requested limit to `1..=max`, defaulting to `default`.
pub fn clamp_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    match limit {
        None => default,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(max),
    }
}

/// Build the common MCP page object and encode an optional service cursor.
pub fn page_json<C>(
    limit: i64,
    returned: usize,
    has_more: bool,
    next_cursor: Option<&C>,
) -> Result<Value, ErrorData>
where
    C: Serialize,
{
    let next_cursor = match next_cursor {
        Some(cursor) => Some(encode_cursor(cursor)?),
        None => None,
    };

    Ok(json!({
        "limit": limit,
        "returned": returned,
        "has_more": has_more,
        "next_cursor": next_cursor,
    }))
}
