mod parser;

use std::collections::HashMap;
use std::time::Duration;

use futures_util::stream::{self, StreamExt as _};
use notegate_core::{Error, Result, limits};
use notegate_db::{
    FilesRepo, LINK_GRAPH_PROJECT_BATCH_MAX,
    LinkGraphNodeRequestOutcome as DbLinkGraphNodeRequestOutcome, LinkGraphProjectSource,
    LinkGraphProjection, LinkGraphProjectionClaim, LinkGraphRepo, LinkGraphSourceSnapshot,
    LinkGraphSpaceRequestOutcome as DbLinkGraphSpaceRequestOutcome, LinkGraphStoredReference,
    LinkGraphWorkRepo,
};
use notegate_jobs::ClaimFence;
use notegate_model::{
    AccountKind, Caller, Channel, IncomingLinkCursor, LinkReference, LinkReferencePage,
    ListLinkReferences, NodeLinkGraphState, OutgoingLinkCursor, Permission, TextStorageFormat,
};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::pagination::paginate_keyset;

use parser::{ParseInternalReferencesError, parse_internal_references};

const LINK_REFERENCE_LIMIT_FAILURE_CODE: &str = "link_reference_limit_exceeded";
const LINK_GRAPH_SOURCE_CONCURRENCY: usize = 10;
const LINK_GRAPH_SOURCE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkGraphProjectionBatch {
    pub projected: usize,
    pub failed: usize,
    pub removed: usize,
    pub skipped: usize,
    pub stale: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphSpaceRequestOutcome {
    Requested,
    AlreadyPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphNodeRequestOutcome {
    Requested,
    AlreadyPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkGraphRequestEligibility {
    Available,
    Forbidden,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLinkGraphView {
    pub state: NodeLinkGraphState,
    pub request_eligibility: LinkGraphRequestEligibility,
    pub request_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceLinkGraphView {
    pub pending: bool,
    pub request_eligibility: LinkGraphRequestEligibility,
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

    pub async fn node_view(
        &self,
        caller: &Caller,
        space_id: Uuid,
        node_id: Uuid,
    ) -> ServiceResult<NodeLinkGraphView> {
        let permission = self
            .files
            .permission_for(space_id, caller.account_id())
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        self.files
            .find_node(space_id, node_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("node not found".to_owned()))?;
        let text = self.files.text_stats(space_id, node_id).await?;
        let request_eligibility = if caller.account.kind != AccountKind::User
            || caller.channel != Channel::Browser
            || permission != Permission::Write
        {
            LinkGraphRequestEligibility::Forbidden
        } else if text
            .as_ref()
            .is_none_or(|text| text.storage_format == TextStorageFormat::Encrypted)
        {
            LinkGraphRequestEligibility::Unsupported
        } else {
            LinkGraphRequestEligibility::Available
        };
        let state = self.store.state(space_id, node_id).await?;
        let request_pending = self.work.node_request_pending(space_id, node_id).await?;
        Ok(NodeLinkGraphView {
            state,
            request_eligibility,
            request_pending,
        })
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
    ) -> ServiceResult<LinkGraphNodeRequestOutcome> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        let text = self.files.text_stats(space_id, node_id).await?;
        if text.is_none() {
            return Err(ServiceError::NotFound("text not found".to_owned()));
        }
        if text.is_some_and(|text| text.storage_format == TextStorageFormat::Encrypted) {
            return Err(ServiceError::InvalidInput(
                "client-encrypted text cannot be link indexed".to_owned(),
            ));
        }
        match self.work.request_node(space_id, node_id).await? {
            DbLinkGraphNodeRequestOutcome::Requested => Ok(LinkGraphNodeRequestOutcome::Requested),
            DbLinkGraphNodeRequestOutcome::AlreadyPending => {
                Ok(LinkGraphNodeRequestOutcome::AlreadyPending)
            }
        }
    }

    pub async fn space_view(
        &self,
        caller: &Caller,
        space_id: Uuid,
    ) -> ServiceResult<SpaceLinkGraphView> {
        require_dashboard_user(caller)?;
        let permission = self
            .files
            .permission_for(space_id, caller.account_id())
            .await?;
        let permission =
            permission.ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        let pending = self
            .work
            .space_pending(space_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("space not found".to_owned()))?;
        Ok(SpaceLinkGraphView {
            pending,
            request_eligibility: if permission == Permission::Write {
                LinkGraphRequestEligibility::Available
            } else {
                LinkGraphRequestEligibility::Forbidden
            },
        })
    }

    pub async fn request_space(
        &self,
        caller: &Caller,
        space_id: Uuid,
    ) -> ServiceResult<LinkGraphSpaceRequestOutcome> {
        require_dashboard_user(caller)?;
        self.require_permission(caller.account_id(), space_id, Permission::Write)
            .await?;
        match self.work.request_space(space_id).await? {
            DbLinkGraphSpaceRequestOutcome::Requested => {
                Ok(LinkGraphSpaceRequestOutcome::Requested)
            }
            DbLinkGraphSpaceRequestOutcome::AlreadyPending => {
                Ok(LinkGraphSpaceRequestOutcome::AlreadyPending)
            }
            DbLinkGraphSpaceRequestOutcome::NotFound => {
                Err(ServiceError::NotFound("space not found".to_owned()))
            }
        }
    }

    pub async fn project_job(
        &self,
        fence: ClaimFence,
        space_id: Uuid,
        sources: &[LinkGraphProjectSource],
    ) -> Result<LinkGraphProjectionBatch> {
        if sources.is_empty() || sources.len() > LINK_GRAPH_PROJECT_BATCH_MAX {
            return Err(Error::validation(format!(
                "link projection batch must contain between 1 and {LINK_GRAPH_PROJECT_BATCH_MAX} node ids"
            )));
        }
        let source_snapshots = sources
            .iter()
            .map(|source| (source.node_id, source.expected_content_sha256.clone()))
            .collect::<HashMap<_, _>>();
        if source_snapshots.len() != sources.len() {
            return Err(Error::validation(
                "link projection batch contains duplicate node ids",
            ));
        }
        let node_ids = sources
            .iter()
            .map(|source| source.node_id)
            .collect::<Vec<_>>();

        let targets = self
            .work
            .claimed_targets(fence.job_id, space_id, &node_ids)
            .await?;
        let mut result = LinkGraphProjectionBatch::default();
        let mut first_error = None;
        let outcomes = stream::iter(targets)
            .map(|target| {
                let expected_content_sha256 = source_snapshots.get(&target.node_id).cloned();
                async move {
                    let expected_content_sha256 = expected_content_sha256.ok_or_else(|| {
                        Error::internal(format!(
                            "claimed link source {} is missing from the job payload",
                            target.node_id
                        ))
                    })?;
                    let claim = LinkGraphProjectionClaim {
                        fence,
                        request_version: target.request_version,
                    };
                    tokio::time::timeout(
                        LINK_GRAPH_SOURCE_TIMEOUT,
                        self.project_target(
                            space_id,
                            target.node_id,
                            claim,
                            expected_content_sha256.as_deref(),
                        ),
                    )
                    .await
                    .map_err(|_elapsed| {
                        Error::internal(format!(
                            "link source projection timed out: {}",
                            target.node_id
                        ))
                    })?
                }
            })
            .buffer_unordered(LINK_GRAPH_SOURCE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        for outcome in outcomes {
            match outcome {
                Ok(projection) => record_projection(&mut result, projection),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        let dispatch_result = self.work.dispatch_ready_nodes(space_id, &node_ids).await;
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
        expected_content_sha256: Option<&str>,
    ) -> Result<LinkGraphProjection> {
        let text = self.files.find_text_object(space_id, node_id).await?;
        if text.as_ref().map(|text| text.content_sha256.as_str()) != expected_content_sha256 {
            return self
                .store
                .settle_stale_target(space_id, node_id, claim)
                .await;
        }
        let Some(text) = text else {
            let projection = self
                .store
                .reconcile_non_text_node(space_id, node_id, claim)
                .await?;
            return match projection {
                LinkGraphProjection::Removed | LinkGraphProjection::Stale => Ok(projection),
                LinkGraphProjection::Applied
                | LinkGraphProjection::Failed
                | LinkGraphProjection::Skipped => Err(Error::internal(
                    "non-text node produced an invalid link projection result",
                )),
            };
        };
        if text.storage_format == TextStorageFormat::Encrypted {
            let projection = self
                .store
                .cleanup_encrypted_source(space_id, node_id, claim, &text.content_sha256)
                .await?;
            return match projection {
                LinkGraphProjection::Skipped
                | LinkGraphProjection::Removed
                | LinkGraphProjection::Stale => Ok(projection),
                LinkGraphProjection::Applied | LinkGraphProjection::Failed => Err(Error::internal(
                    "encrypted text produced an invalid link projection result",
                )),
            };
        }
        let source_path = self
            .files
            .node_path(space_id, node_id)
            .await?
            .ok_or_else(|| Error::internal("live link source has no path"))?;
        let content = text
            .content
            .as_deref()
            .ok_or_else(|| Error::internal("link source has no readable text content"))?;
        let references = match parse_internal_references(&source_path, content) {
            Ok(references) => references,
            Err(ParseInternalReferencesError::TooManyReferences { .. }) => {
                return self
                    .store
                    .fail_projection_target(
                        space_id,
                        node_id,
                        claim,
                        LINK_REFERENCE_LIMIT_FAILURE_CODE,
                        &text.content_sha256,
                        &source_path,
                    )
                    .await;
            }
        }
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
                node_id,
                claim,
                LinkGraphSourceSnapshot {
                    content_sha256: &text.content_sha256,
                    path: &source_path,
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
        LinkGraphProjection::Applied => result.projected += 1,
        LinkGraphProjection::Failed => result.failed += 1,
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
