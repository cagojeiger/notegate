//! Thin `record` helper used by the file-tree commands to log mutations to
//! `file_change_events` without repeating row-construction boilerplate.

use crate::file_change_event_repo::{NewFileChangeEvent, insert_file_change_event};
use notegate_core::Result;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileChangeContext {
    actor_account_id: Uuid,
    space_id: Uuid,
}

pub(crate) fn context(actor_account_id: Uuid, space_id: Uuid) -> FileChangeContext {
    FileChangeContext {
        actor_account_id,
        space_id,
    }
}

pub(crate) async fn record(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Option<Uuid>,
    op_type: &'static str,
    metadata: Value,
) -> Result<()> {
    insert_file_change_event(
        tx,
        NewFileChangeEvent {
            space_id: ctx.space_id,
            node_id,
            actor_account_id: Some(ctx.actor_account_id),
            op_type,
            metadata,
        },
    )
    .await
}
