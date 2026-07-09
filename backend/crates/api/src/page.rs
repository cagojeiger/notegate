//! Shared pagination output used by REST and MCP surfaces.
//!
//! Services own cursor semantics. API surfaces only serialize the already
//! opaque cursor string returned by the service.

use serde::Serialize;
use utoipa::ToSchema;

/// Pagination metadata returned by every list/search response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Page {
    pub limit: i64,
    pub returned: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

impl Page {
    /// Build a page object from service pagination output and the number of
    /// items actually serialized by the surface.
    pub fn new(limit: i64, returned: usize, has_more: bool, next_cursor: Option<String>) -> Self {
        Self {
            limit,
            returned: returned as i64,
            has_more,
            next_cursor,
        }
    }

    pub fn from_items<T>(
        limit: i64,
        items: &[T],
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self::new(limit, items.len(), has_more, next_cursor)
    }
}
