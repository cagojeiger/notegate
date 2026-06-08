//! Shared helpers for MCP tool handlers.

use serde_json::{Value, json};

use crate::page::Page;

/// Build the common MCP page object. Cursors are already opaque service strings.
pub fn page_json(limit: i64, returned: usize, has_more: bool, next_cursor: Option<&str>) -> Value {
    json!(Page::new(
        limit,
        returned,
        has_more,
        next_cursor.map(str::to_owned),
    ))
}
