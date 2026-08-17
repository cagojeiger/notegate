mod parser;

use notegate_core::{Error, Result, limits};
use notegate_db::{
    FilesRepo, LINK_GRAPH_PROJECT_BATCH_MAX, LinkGraphProjection, LinkGraphProjectionClaim,
    LinkGraphRepo, LinkGraphSourceSnapshot, LinkGraphStoredReference, LinkGraphWorkRepo,
};
use notegate_jobs::ClaimFence;
use notegate_model::{
    AccountKind, Caller, Channel, IncomingLinkCursor, LinkReference, LinkReferencePage,
    ListLinkReferences, NodeLinkGraphState, OutgoingLinkCursor, Permission, TextStorageFormat,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::paginate_keyset;

use parser::parse_internal_references;

pub const LINK_GRAPH_PARSER_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkGraphProjectionBatch {
    pub projected: usize,
    pub removed: usize,
    pub skipped: usize,
    pub stale: usize,
}

#[derive(Debug, Clone)]
pub struct LinkGraphService {
    store: LinkGraphRepo,
    files: FilesRepo,
    work: LinkGraphWorkRepo,
}

impl LinkGraphService {
    pub fn new(store: LinkGraphRepo, files: FilesRepo, work: LinkGraphWorkRepo) -> Self {
        Self { store, files, work }
    }

    pub async fn node_state(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeLinkGraphState> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        self.store
            .state(space_id, node_id)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn outgoing(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        request: ListLinkReferences,
    ) -> ServiceResult<LinkReferencePage> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        let (references, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::LINK_REFERENCES_DEFAULT_LIMIT,
            limits::LINK_REFERENCES_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<OutgoingLinkCursor>| async move {
                if cursor.as_ref().is_some_and(|cursor| {
                    cursor.space_id != space_id || cursor.source_node_id != node_id
                }) {
                    return Err(ServiceError::InvalidInput(
                        "cursor does not match outgoing links query".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .outgoing(space_id, node_id, limit, cursor.as_ref())
                    .await?)
            },
            |reference| OutgoingLinkCursor {
                space_id,
                source_node_id: node_id,
                kind: reference.kind,
                target_path: reference.target_path.clone(),
            },
        )
        .await?;
        let items = references
            .into_iter()
            .map(|reference| LinkReference {
                node_id: reference.target_node_id,
                path: reference.target_path,
                kind: reference.kind,
                occurrence_count: reference.occurrence_count,
            })
            .collect();
        Ok(LinkReferencePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn incoming(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
        request: ListLinkReferences,
    ) -> ServiceResult<LinkReferencePage> {
        self.require_visible_node(caller_account_id, space_id, node_id)
            .await?;
        let (references, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::LINK_REFERENCES_DEFAULT_LIMIT,
            limits::LINK_REFERENCES_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<IncomingLinkCursor>| async move {
                if cursor.as_ref().is_some_and(|cursor| {
                    cursor.space_id != space_id || cursor.target_node_id != node_id
                }) {
                    return Err(ServiceError::InvalidInput(
                        "cursor does not match incoming links query".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .incoming(space_id, node_id, limit, cursor.as_ref())
                    .await?)
            },
            |reference| IncomingLinkCursor {
                space_id,
                target_node_id: node_id,
                source_node_id: reference.source_node_id,
                kind: reference.kind,
            },
        )
        .await?;
        let source_ids = references
            .iter()
            .map(|reference| reference.source_node_id)
            .collect::<Vec<_>>();
        let source_paths = self.files.node_paths_many(space_id, &source_ids).await?;
        let items = references
            .into_iter()
            .filter_map(|reference| {
                source_paths
                    .get(&reference.source_node_id)
                    .map(|path| LinkReference {
                        node_id: Some(reference.source_node_id),
                        path: path.clone(),
                        kind: reference.kind,
                        occurrence_count: reference.occurrence_count,
                    })
            })
            .collect();
        Ok(LinkReferencePage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn request_node(
        &self,
        caller: &Caller,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        if self.files.find_text(space_id, node_id).await?.is_none() {
            return Err(ServiceError::NotFound("text not found".to_owned()));
        }
        self.work.request_nodes(space_id, &[node_id]).await?;
        Ok(())
    }

    pub async fn request_space(&self, caller: &Caller, space_id: Uuid) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        if !self.work.request_space(space_id).await? {
            return Err(ServiceError::NotFound("space not found".to_owned()));
        }
        Ok(())
    }

    pub async fn project_job(
        &self,
        fence: ClaimFence,
        space_id: Uuid,
        node_ids: &[Uuid],
    ) -> Result<LinkGraphProjectionBatch> {
        if node_ids.is_empty() || node_ids.len() > LINK_GRAPH_PROJECT_BATCH_MAX {
            return Err(Error::validation(format!(
                "link projection batch must contain between 1 and {LINK_GRAPH_PROJECT_BATCH_MAX} node ids"
            )));
        }

        let targets = self
            .work
            .claimed_targets(fence.job_id, space_id, node_ids)
            .await?;
        let mut result = LinkGraphProjectionBatch::default();
        let mut first_error = None;
        for target in targets {
            let claim = LinkGraphProjectionClaim {
                fence,
                request_version: target.request_version,
            };
            match self.project_target(space_id, target.node_id, claim).await {
                Ok(projection) => record_projection(&mut result, projection),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        let dispatch_result = self.work.dispatch_ready_nodes(space_id, node_ids).await;
        match (first_error, dispatch_result) {
            (Some(error), _) => Err(error),
            (None, Err(error)) => Err(error),
            (None, Ok(())) => Ok(result),
        }
    }

    async fn project_target(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        claim: LinkGraphProjectionClaim,
    ) -> Result<LinkGraphProjection> {
        let Some((node, text)) = self.files.find_text(space_id, node_id).await? else {
            let projection = self
                .store
                .reconcile_non_text_node(space_id, node_id, claim)
                .await?;
            return match projection {
                LinkGraphProjection::Removed | LinkGraphProjection::Stale => Ok(projection),
                LinkGraphProjection::Applied { .. } | LinkGraphProjection::Skipped => Err(
                    Error::internal("non-text node produced an invalid link projection result"),
                ),
            };
        };
        if text.storage_format == TextStorageFormat::Encrypted {
            let projection = self
                .store
                .cleanup_encrypted_source(space_id, node.id, claim, &text.content_sha256)
                .await?;
            return match projection {
                LinkGraphProjection::Skipped
                | LinkGraphProjection::Removed
                | LinkGraphProjection::Stale => Ok(projection),
                LinkGraphProjection::Applied { .. } => Err(Error::internal(
                    "encrypted text produced an invalid link projection result",
                )),
            };
        }
        let source_path = self
            .files
            .node_path(space_id, node.id)
            .await?
            .ok_or_else(|| Error::internal("live link source has no path"))?;
        let content = text
            .content
            .as_deref()
            .ok_or_else(|| Error::internal("link source has no readable text content"))?;
        let references = parse_internal_references(&source_path, content)
            .into_iter()
            .map(|reference| LinkGraphStoredReference {
                target_path: reference.target_path,
                kind: reference.kind,
                occurrence_count: reference.occurrence_count,
            })
            .collect::<Vec<_>>();
        self.store
            .replace_source(
                space_id,
                node.id,
                claim,
                LinkGraphSourceSnapshot {
                    content_sha256: &text.content_sha256,
                    path: &source_path,
                    parser_version: LINK_GRAPH_PARSER_VERSION,
                    references: &references,
                },
            )
            .await
    }

    async fn require_permission(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        required: Permission,
    ) -> ServiceResult<()> {
        let permission = self
            .files
            .permission_for(space_id, caller_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        if required == Permission::Write && permission != Permission::Write {
            return Err(ServiceError::Forbidden(
                "write permission is required".to_owned(),
            ));
        }
        Ok(())
    }

    async fn require_visible_node(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<()> {
        self.require_permission(caller_account_id, space_id, Permission::Read)
            .await?;
        self.files
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))?;
        Ok(())
    }
}

fn record_projection(result: &mut LinkGraphProjectionBatch, projection: LinkGraphProjection) {
    match projection {
        LinkGraphProjection::Applied { .. } => result.projected += 1,
        LinkGraphProjection::Removed => result.removed += 1,
        LinkGraphProjection::Skipped => result.skipped += 1,
        LinkGraphProjection::Stale => result.stale += 1,
    }
}

fn require_dashboard_user(caller: &Caller) -> ServiceResult<()> {
    if caller.account.kind != AccountKind::User || caller.channel != Channel::Browser {
        return Err(ServiceError::Forbidden(
            "link indexing can only be requested from the dashboard".to_owned(),
        ));
    }
    Ok(())
}
