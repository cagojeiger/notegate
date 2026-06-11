//! `grep`: text-content search that returns matching text nodes.

use notegate_core::limits;
use notegate_model::files::NodeView;

use crate::cursor;
use crate::error::{ServiceError, ServiceResult};
use crate::files::policy::FileCommand;
use crate::files::validation;
use crate::pagination::clamp_limit;

use super::{GrepCursor, GrepPage, GrepRequest, SearchService, validate_query};

impl SearchService {
    /// Grep text content: return text nodes whose plain content contains `q`.
    ///
    /// Authorization mirrors file reads (`grep` requires read permission; no
    /// permission ⇒ `404`). The page limit is clamped to
    /// `1..=GREP_MAX_LIMIT` (default `GREP_DEFAULT_LIMIT`). A malformed cursor is
    /// a clean `400`-class [`ServiceError::InvalidInput`].
    pub async fn grep(
        &self,
        caller_account_id: uuid::Uuid,
        space_id: uuid::Uuid,
        request: GrepRequest,
    ) -> ServiceResult<GrepPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Grep)
            .await?;
        let q = validate_query(&request.q)?;
        let limit = clamp_limit(
            request.limit,
            limits::GREP_DEFAULT_LIMIT,
            limits::GREP_MAX_LIMIT,
        );
        let cursor: Option<GrepCursor> = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(cursor::decode(raw)?),
        };
        let scope_path = request
            .path
            .as_deref()
            .map(validation::normalize_path)
            .transpose()?;

        let mut candidates = self
            .store
            .grep_candidates(
                space_id,
                q,
                scope_path.as_deref(),
                limit + 1,
                cursor.as_ref(),
            )
            .await?;
        let has_more = candidates.len() as i64 > limit;
        candidates.truncate(limit as usize);

        let next_cursor = if has_more {
            candidates
                .last()
                .map(|candidate| GrepCursor {
                    updated_at: candidate.updated_at,
                    node_id: candidate.node.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        let items = candidates
            .into_iter()
            .map(|candidate| NodeView {
                node: candidate.node,
                path: candidate.path,
                has_children: candidate.has_children,
                text: Some(candidate.text),
                file: None,
            })
            .collect();

        Ok(GrepPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}
