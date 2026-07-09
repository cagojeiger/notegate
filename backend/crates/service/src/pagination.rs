//! Shared pagination policy for service commands.
//!
//! Transports pass opaque cursor strings through unchanged. Services clamp
//! limits, decode incoming cursors into server-owned positions, and encode
//! outgoing cursors back to opaque strings.

use std::future::Future;

use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};

/// Clamp a requested page limit to `1..=max`, defaulting when omitted.
pub(crate) fn clamp_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    match limit {
        None => default,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(max),
    }
}

/// Keyset-page a small, fully materialized, stably ordered list by item id.
pub(crate) fn paginate_by_id<T>(
    items: Vec<T>,
    id_of: impl Fn(&T) -> Uuid,
    limit: i64,
    cursor: Option<&str>,
) -> ServiceResult<(Vec<T>, bool, Option<String>)> {
    let start = match cursor {
        None => 0,
        Some(raw) => {
            let after: Uuid = cursor::decode(raw)?;
            items
                .iter()
                .position(|item| id_of(item) == after)
                .map(|index| index + 1)
                .unwrap_or(items.len())
        }
    };

    let mut window: Vec<T> = items
        .into_iter()
        .skip(start)
        .take(limit as usize + 1)
        .collect();
    let has_more = window.len() as i64 > limit;
    if has_more {
        window.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        window
            .last()
            .map(|item| cursor::encode(&id_of(item)))
            .transpose()
            .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
    } else {
        None
    };
    Ok((window, has_more, next_cursor))
}

/// Page a database-backed keyset query that returns `limit + 1` rows.
pub(crate) async fn paginate_keyset<T, C, F, Fut>(
    requested_limit: Option<i64>,
    default_limit: i64,
    max_limit: i64,
    raw_cursor: Option<&str>,
    fetch: F,
    cursor_of: impl Fn(&T) -> C,
) -> ServiceResult<(Vec<T>, i64, bool, Option<String>)>
where
    C: Serialize + DeserializeOwned,
    F: FnOnce(i64, Option<C>) -> Fut,
    Fut: Future<Output = ServiceResult<Vec<T>>>,
{
    let limit = clamp_limit(requested_limit, default_limit, max_limit);
    let cursor = raw_cursor.map(cursor::decode::<C>).transpose()?;

    let mut items = fetch(limit + 1, cursor).await?;
    let has_more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next_cursor = if has_more {
        items
            .last()
            .map(cursor_of)
            .map(|cursor| cursor::encode(&cursor))
            .transpose()
            .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
    } else {
        None
    };

    Ok((items, limit, has_more, next_cursor))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn clamp_limit_uses_service_policy() {
        assert_eq!(clamp_limit(None, 50, 100), 50, "absent → default");
        assert_eq!(clamp_limit(Some(0), 50, 100), 1, "below 1 → 1");
        assert_eq!(clamp_limit(Some(250), 50, 100), 100, "above max → max");
        assert_eq!(clamp_limit(Some(30), 50, 100), 30, "in range → as-is");
    }
}
