//! Shared helpers for MCP tool handlers.

use serde_json::{Value, json};

/// Build the common MCP page object. Cursors are already opaque service strings.
pub fn page_json(limit: i64, returned: usize, has_more: bool, next_cursor: Option<&str>) -> Value {
    json!({
        "limit": limit,
        "returned": returned,
        "has_more": has_more,
        "next_cursor": next_cursor,
    })
}
