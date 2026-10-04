//! Version authorization and guarded restore through the ordinary write path.
use notegate_model::text_revision::{TextRevision, TextRevisionContent, TextRevisionCursor};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{FileCommand, FilesService, TextView, WriteTarget, WriteText, WriteTextBody};
use crate::{ServiceError, ServiceResult, cursor};

#[derive(Debug, Serialize)]
pub struct RevisionHistoryPage {
    pub revisions: Vec<TextRevision>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct HistoryCursor {
    space_id: Uuid,
    node_id: Uuid,
    position: TextRevisionCursor,
}

impl FilesService {
    pub fn with_revision_session(mut self, session: Option<Uuid>) -> Self {
        self.store = self.store.with_revision_context(
            match self.channel {
                notegate_model::Channel::Browser => "browser",
                notegate_model::Channel::Api => "api",
                notegate_model::Channel::Mcp => "mcp",
            },
            session,
        );
        self
    }

    pub async fn text_revisions(
        &self,
        actor: Uuid,
        space: Uuid,
        node: Uuid,
        limit: i64,
        raw_cursor: Option<&str>,
    ) -> ServiceResult<RevisionHistoryPage> {
        self.authorize(space, actor, FileCommand::Read).await?;
        self.load_node(space, node).await?;
        let decoded: Option<HistoryCursor> = raw_cursor.map(cursor::decode).transpose()?;
        if decoded
            .as_ref()
            .is_some_and(|c| c.space_id != space || c.node_id != node)
        {
            return Err(ServiceError::InvalidInput(
                "revision cursor belongs to another document".to_owned(),
            ));
        }
        let page = self
            .store
            .list_text_revisions(space, node, limit, decoded.as_ref().map(|c| &c.position))
            .await?;
        let next_cursor = page
            .next_cursor
            .map(|position| {
                cursor::encode(&HistoryCursor {
                    space_id: space,
                    node_id: node,
                    position,
                })
            })
            .transpose()?;
        Ok(RevisionHistoryPage {
            revisions: page.revisions,
            next_cursor,
        })
    }

    pub async fn text_revision(
        &self,
        actor: Uuid,
        space: Uuid,
        node: Uuid,
        revision: Uuid,
    ) -> ServiceResult<TextRevisionContent> {
        self.authorize(space, actor, FileCommand::Read).await?;
        self.load_node(space, node).await?;
        Ok(self.store.read_text_revision(space, node, revision).await?)
    }

    pub async fn restore_text_revision(
        &self,
        actor: Uuid,
        space: Uuid,
        node: Uuid,
        revision: Uuid,
        expected_sha256: String,
    ) -> ServiceResult<TextView> {
        self.authorize(space, actor, FileCommand::Write).await?;
        if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(ServiceError::InvalidInput(
                "expected_sha256 must be a SHA-256 hex digest".to_owned(),
            ));
        }
        let previous = self.text_revision(actor, space, node, revision).await?;
        let mut restoring = self.clone();
        restoring.store = restoring.store.with_revision_context("restore", None);
        restoring
            .write_text(
                actor,
                space,
                WriteText {
                    target: WriteTarget::Existing { node_id: node },
                    body: WriteTextBody::Plain(previous.content),
                    expected_sha256: Some(expected_sha256),
                },
            )
            .await
    }
}
