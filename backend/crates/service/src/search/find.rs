//! `find`: node-name metadata search, keyset-paginated by `(name, id)`.

use notegate_core::limits;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::NodeView;
use crate::files::policy::FileCommand;
use crate::files::validation;
use crate::pagination::clamp_limit;

use super::{FindCursor, FindPage, FindRequest, SearchService, validate_query};

impl SearchService {
    /// Find nodes by name, optionally filtered by `kind` and scoped to a path's
    /// subtree. Keyset-paginated by `(name, id)`.
    ///
    /// Authorization mirrors file reads: the caller's live space permission is
    /// resolved first (no permission ⇒ `404`, which hides the space); `find`
    /// requires read permission. The limit is clamped to `1..=FIND_MAX_LIMIT` (default
    /// `FIND_DEFAULT_LIMIT`); a malformed cursor is a clean `400`-class
    /// [`ServiceError::InvalidInput`].
    pub async fn find(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: FindRequest,
    ) -> ServiceResult<FindPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Find)
            .await?;
        let q = validate_query(&request.q)?;
        let limit = clamp_limit(
            request.limit,
            limits::FIND_DEFAULT_LIMIT,
            limits::FIND_MAX_LIMIT,
        );

        // Decode the opaque cursor (garbage/tampered → InvalidInput → 400).
        let cursor: Option<FindCursor> = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode(raw)?),
        };

        let scope_path = request
            .path
            .as_deref()
            .map(validation::normalize_path)
            .transpose()?;

        // Fetch `limit + 1` to detect a next page without a second query.
        let rows = self
            .store
            .find_nodes(
                space_id,
                q,
                scope_path.as_deref(),
                request.kind,
                limit + 1,
                cursor.as_ref(),
            )
            .await?;

        let has_more = rows.len() as i64 > limit;
        let mut rows = rows;
        rows.truncate(limit as usize);

        // The next cursor is the LAST returned row's `(name, id)` keyset.
        let next_cursor = if has_more {
            rows.last()
                .map(|(node, _path, _has_children)| FindCursor {
                    name: node.name.clone(),
                    id: node.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        let items = rows
            .into_iter()
            .map(|(node, path, has_children)| NodeView {
                node,
                path,
                has_children,
                text: None,
                file: None,
            })
            .collect();

        Ok(FindPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}
