//! Browser-facing Markdown link-index reads and rebuild requests.

mod markdown;
mod projector;

use notegate_db::{FilesRepo, LinkIndexRepo, LinkReferenceRecord, NodeLinkRecords};
use notegate_model::{
    LinkIndexFreshness, LinkReference, LinkReferenceStatus, NodeLinkSummary, Permission,
    SpaceLinkIndexState,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

pub use projector::{LinkIndexProjector, LinkIndexRun};

pub const NODE_LINK_PREVIEW_LIMIT: i64 = 8;

#[derive(Debug, Clone)]
pub struct LinkIndexService {
    index: LinkIndexRepo,
    files: FilesRepo,
}

impl LinkIndexService {
    pub fn new(index: LinkIndexRepo, files: FilesRepo) -> Self {
        Self { index, files }
    }

    pub async fn state(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkIndexState> {
        self.authorize(caller_account_id, space_id, false).await?;
        self.index
            .state(space_id)
            .await?
            .ok_or_else(|| ServiceError::Internal("link index state is missing".to_owned()))
    }

    pub async fn request_rebuild(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkIndexState> {
        self.authorize(caller_account_id, space_id, true).await?;
        Ok(self.index.request_rebuild(space_id).await?)
    }

    pub async fn node_links(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeLinkSummary> {
        self.authorize(caller_account_id, space_id, false).await?;
        let records = self
            .index
            .node_links(space_id, node_id, NODE_LINK_PREVIEW_LIMIT)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))?;
        Ok(hydrate_records(records))
    }

    async fn authorize(
        &self,
        account_id: Uuid,
        space_id: Uuid,
        write: bool,
    ) -> ServiceResult<Permission> {
        let permission = self
            .files
            .permission_for(space_id, account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        if write && !permission.allows_write() {
            return Err(ServiceError::Forbidden(
                "link reindexing requires write permission".to_owned(),
            ));
        }
        Ok(permission)
    }
}

fn hydrate_records(records: NodeLinkRecords) -> NodeLinkSummary {
    if matches!(
        records.index.freshness(),
        LinkIndexFreshness::Rebuilding | LinkIndexFreshness::Failed
    ) {
        return NodeLinkSummary {
            index: records.index,
            outgoing_count: 0,
            incoming_count: 0,
            broken_count: 0,
            outgoing: Vec::new(),
            incoming: Vec::new(),
            outgoing_truncated: false,
            incoming_truncated: false,
        };
    }

    let NodeLinkRecords {
        index,
        outgoing_count,
        incoming_count,
        broken_count,
        outgoing,
        incoming,
        outgoing_truncated,
        incoming_truncated,
    } = records;
    NodeLinkSummary {
        index,
        outgoing_count,
        incoming_count,
        broken_count,
        outgoing: outgoing.into_iter().map(hydrate_reference).collect(),
        incoming: incoming.into_iter().map(hydrate_reference).collect(),
        outgoing_truncated,
        incoming_truncated,
    }
}

fn hydrate_reference(record: LinkReferenceRecord) -> LinkReference {
    let status = if record.normalized_target_path.is_none() {
        LinkReferenceStatus::Invalid
    } else if record.target_node_id.is_none() {
        LinkReferenceStatus::Missing
    } else if record.target_deleted {
        LinkReferenceStatus::Deleted
    } else {
        LinkReferenceStatus::Resolved
    };
    LinkReference {
        id: record.id,
        kind: record.kind,
        status,
        raw_href: record.raw_href,
        normalized_target_path: record.normalized_target_path,
        occurrence_count: record.occurrence_count,
        source_node_id: record.source_node_id,
        source_name: record.source_name,
        source_path: record.source_path,
        target_node_id: record.target_node_id,
        target_name: record.target_name,
        target_path: record.target_path,
    }
}
