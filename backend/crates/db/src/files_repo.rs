//! Nodes + text-object persistence (the file tree) and search.
//!
//! Exposes concrete persistence methods consumed by `notegate-service` and `notegate-search`.
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never the
//! `query!` macro — so a schema reset never breaks compilation. Every mutation
//! runs in one transaction and sets attribution (created_by/updated_by/
//! deleted_by) from the caller.
//!
//! Nodes have NO stored path column. Display paths and scoped search paths are
//! derived by space-bounded recursive CTEs;
//! move/rename updates only the moved node's row (O(1), no descendant rewrite).

#[path = "files/mod.rs"]
pub mod files;

#[path = "files/trash.rs"]
mod trash;

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_core::limits::Limits;
use notegate_core::security::PiiCrypto;
use notegate_model::{FileObject, Node, SavedText};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use self::files::commands;
use crate::file_change_event_repo;
use notegate_model::files::{
    BeginObjectUpload, CopyCounts, CopyNode, CreateFolder, FileMutationContext, MoveNode,
    ObjectUploadMode, ObjectUploadRegistration, PendingObjectUpload, StoredContent,
};

#[derive(Debug, Clone)]
pub struct FilesRepo {
    pool: PgPool,
    limits: Limits,
    crypto: PiiCrypto,
    metrics_enabled: bool,
    external_only: bool,
    mutation_context: FileMutationContext,
    revision_source_override: Option<&'static str>,
    revision_time: Option<DateTime<Utc>>,
    trash_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy)]
pub enum TextMutationKind {
    Write,
    Append,
    Patch,
    Edit,
}

impl TextMutationKind {
    pub(crate) fn op_type(self) -> &'static str {
        match self {
            TextMutationKind::Write => "text.write",
            TextMutationKind::Append => "text.append",
            TextMutationKind::Patch => "text.patch",
            TextMutationKind::Edit => "text.edit",
        }
    }
}

impl FilesRepo {
    #[cfg(any(test, feature = "test-util"))]
    pub fn new(pool: PgPool) -> Self {
        Self::with_limits(pool, Limits::default())
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn with_limits(pool: PgPool, limits: Limits) -> Self {
        Self::with_limits_and_crypto(pool, limits, PiiCrypto::test())
    }

    pub fn with_limits_and_crypto(pool: PgPool, limits: Limits, crypto: PiiCrypto) -> Self {
        Self {
            pool,
            limits,
            crypto,
            metrics_enabled: false,
            external_only: false,
            mutation_context: FileMutationContext::default(),
            revision_source_override: None,
            revision_time: None,
            trash_time: None,
        }
    }

    pub fn with_external_access_only(mut self, external_only: bool) -> Self {
        self.external_only = external_only;
        self
    }

    /// Replace all request attribution together; authorization is independent.
    pub fn with_mutation_context(mut self, context: FileMutationContext) -> Self {
        self.mutation_context = context;
        self.revision_source_override = None;
        self
    }

    /// Restore starts a new revision group while retaining its transport and invocation.
    pub fn for_revision_restore(mut self) -> Self {
        self.revision_source_override = Some("restore");
        self.mutation_context.edit_session_id = None;
        self.mutation_context.purpose = None;
        self
    }

    fn revision_source(&self) -> &'static str {
        self.revision_source_override
            .unwrap_or(self.mutation_context.source)
    }

    /// Pin the revision policy clock without changing the database or runtime clock.
    #[cfg(any(test, feature = "test-util"))]
    pub fn with_revision_time(mut self, now: DateTime<Utc>) -> Self {
        self.revision_time = Some(now);
        self
    }

    pub(crate) fn change_capture(&self) -> crate::file_change_events::ChangeCapture<'_> {
        crate::file_change_events::ChangeCapture {
            invocation_id: self.mutation_context.invocation_id,
            crypto: &self.crypto,
            source: self.mutation_context.source,
            purpose: self.mutation_context.purpose.as_deref(),
        }
    }

    pub fn with_metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }
}

impl FilesRepo {
    pub async fn list_file_change_events(
        &self,
        space_id: Uuid,
        node_id: Option<Uuid>,
        limit: i64,
        cursor: Option<&notegate_model::FileChangeEventCursor>,
    ) -> Result<Vec<notegate_model::FileChangeEvent>> {
        file_change_event_repo::list_file_change_events(
            &self.pool,
            &self.crypto,
            space_id,
            node_id,
            limit,
            cursor,
        )
        .await
    }

    pub async fn list_file_change_events_by_id(
        &self,
        space_id: Uuid,
        limit: i64,
        before_id: Option<i64>,
    ) -> Result<Vec<notegate_model::FileChangeEvent>> {
        file_change_event_repo::list_file_change_events_by_id(
            &self.pool,
            &self.crypto,
            space_id,
            limit,
            before_id,
        )
        .await
    }

    pub async fn sync_file_change_events(
        &self,
        space_id: Uuid,
        after_id: Option<i64>,
        limit: i64,
    ) -> Result<crate::FileChangeSyncRows> {
        file_change_event_repo::sync_file_change_events(
            &self.pool,
            &self.crypto,
            space_id,
            after_id,
            limit,
        )
        .await
    }

    pub async fn insert_folder(
        &self,
        space_id: Uuid,
        command: &CreateFolder,
        created_by: Uuid,
    ) -> Result<Node> {
        commands::create::insert_folder(
            &self.pool,
            self.change_capture(),
            self.external_only,
            space_id,
            command.parent_node_id,
            &command.name,
            created_by,
            self.limits,
        )
        .await
    }

    pub async fn insert_text(
        &self,
        space_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        content: &StoredContent,
        created_by: Uuid,
    ) -> Result<(Node, SavedText)> {
        commands::create::insert_text(commands::create::InsertTextArgs {
            pool: &self.pool,
            capture: self.change_capture(),
            external_only: self.external_only,
            crypto: &self.crypto,
            space_id,
            parent_id: parent_node_id,
            name,
            content,
            created_by,
            revision_source: self.revision_source(),
            revision_purpose: self.mutation_context.purpose.as_deref(),
            revision_time: self.revision_time,
            caps: self.limits,
        })
        .await
    }

    pub async fn insert_object_upload(
        &self,
        id: Uuid,
        object_key: &str,
        space_id: Uuid,
        requested_by: Uuid,
        input: &BeginObjectUpload,
    ) -> Result<PendingObjectUpload> {
        let registration = ObjectUploadRegistration {
            id,
            object_key: object_key.to_owned(),
            upload_mode: ObjectUploadMode::Single,
            multipart_upload_id: None,
            multipart_part_size: None,
        };
        self.insert_registered_object_upload(&registration, space_id, requested_by, input)
            .await
    }

    pub async fn insert_registered_object_upload(
        &self,
        registration: &ObjectUploadRegistration,
        space_id: Uuid,
        requested_by: Uuid,
        input: &BeginObjectUpload,
    ) -> Result<PendingObjectUpload> {
        crate::files::object_uploads::insert(
            &self.pool,
            self.external_only,
            registration,
            space_id,
            requested_by,
            input,
            self.limits,
        )
        .await
    }

    pub async fn object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<Option<PendingObjectUpload>> {
        crate::files::object_uploads::find(&self.pool, id, space_id, requested_by).await
    }

    pub async fn object_upload_for_caller(
        &self,
        id: Uuid,
        requested_by: Uuid,
    ) -> Result<Option<PendingObjectUpload>> {
        crate::files::object_uploads::find_for_caller(&self.pool, id, requested_by).await
    }

    pub async fn touch_object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<bool> {
        crate::files::object_uploads::touch(&self.pool, id, space_id, requested_by).await
    }

    pub async fn request_object_upload_expiry(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
    ) -> Result<bool> {
        crate::files::object_uploads::request_expiry(&self.pool, id, space_id, requested_by).await
    }

    pub async fn attach_object_upload(
        &self,
        id: Uuid,
        space_id: Uuid,
        requested_by: Uuid,
        detected_media_type: Option<&str>,
        node_metadata: Option<&Value>,
    ) -> Result<(Node, FileObject)> {
        crate::files::object_uploads::attach(crate::files::object_uploads::AttachUploadArgs {
            pool: &self.pool,
            capture: self.change_capture(),
            external_only: self.external_only,
            id,
            space_id,
            requested_by,
            detected_media_type,
            node_metadata,
            limits: self.limits,
        })
        .await
    }

    pub async fn save_text_content(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        content: &StoredContent,
        expected_sha256: Option<&str>,
        updated_by: Uuid,
        mutation_kind: TextMutationKind,
    ) -> Result<(Node, SavedText)> {
        commands::save::save_text_content(commands::save::SaveTextContentArgs {
            pool: &self.pool,
            capture: self.change_capture(),
            external_only: self.external_only,
            crypto: &self.crypto,
            space_id,
            node_id,
            content,
            expected_sha256,
            updated_by,
            mutation_kind,
            revision_source: self.revision_source(),
            revision_session: self.mutation_context.edit_session_id,
            revision_purpose: self.mutation_context.purpose.as_deref(),
            revision_time: self.revision_time,
            caps: self.limits,
        })
        .await
    }

    pub async fn move_node(
        &self,
        space_id: Uuid,
        command: &MoveNode,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::move_node::move_node(commands::move_node::MoveNodeArgs {
            pool: &self.pool,
            capture: self.change_capture(),
            external_only: self.external_only,
            space_id,
            node_id: command.node_id,
            new_parent_id: command.new_parent_node_id,
            new_name: command.new_name.as_deref(),
            expected_parent_id: command.expected_parent_id,
            updated_by,
            caps: self.limits,
        })
        .await
    }

    pub async fn copy_node(
        &self,
        space_id: Uuid,
        command: &CopyNode,
        created_by: Uuid,
    ) -> Result<(Node, CopyCounts)> {
        commands::copy_node::copy_node(commands::copy_node::CopyNodeArgs {
            pool: &self.pool,
            capture: self.change_capture(),
            external_only: self.external_only,
            space_id,
            source_node_id: command.node_id,
            new_parent_id: command.new_parent_node_id,
            new_name: &command.new_name,
            recursive: command.recursive,
            created_by,
            caps: self.limits,
        })
        .await
    }

    pub async fn update_node(
        &self,
        space_id: Uuid,
        command: &notegate_model::files::UpdateNode,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::update_node(
            &self.pool,
            self.change_capture(),
            self.external_only,
            space_id,
            command,
            updated_by,
        )
        .await
    }

    pub async fn update_node_external_access_policy(
        &self,
        space_id: Uuid,
        command: &notegate_model::files::UpdateNodeExternalAccessPolicy,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::update_node_external_access_policy(
            &self.pool,
            self.change_capture(),
            space_id,
            command,
            updated_by,
        )
        .await
    }

    pub async fn update_node_write_lock(
        &self,
        space_id: Uuid,
        command: &notegate_model::files::UpdateNodeWriteLock,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::write_lock::update_node_write_lock(
            &self.pool,
            self.change_capture(),
            space_id,
            command,
            updated_by,
            self.limits,
        )
        .await
    }

    pub async fn update_text_encryption(
        &self,
        space_id: Uuid,
        command: &notegate_model::files::UpdateTextEncryption,
        updated_by: Uuid,
    ) -> Result<Node> {
        commands::update::update_text_encryption(
            &self.pool,
            self.change_capture(),
            &self.crypto,
            space_id,
            command,
            updated_by,
            self.limits,
        )
        .await
    }

    pub async fn soft_delete_node(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        deleted_by: Uuid,
        recursive: bool,
    ) -> Result<DateTime<Utc>> {
        commands::delete::soft_delete_node(
            &self.pool,
            self.change_capture(),
            self.external_only,
            space_id,
            node_id,
            deleted_by,
            recursive,
        )
        .await
    }
}

impl FilesRepo {
    pub async fn list_text_revisions(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        limit: i64,
        cursor: Option<&notegate_model::text_revision::TextRevisionCursor>,
    ) -> Result<notegate_model::text_revision::TextRevisionPage> {
        crate::files::revisions::list(
            &self.pool,
            &self.crypto,
            space_id,
            node_id,
            limit,
            cursor,
            self.external_only,
        )
        .await
    }

    pub async fn read_text_revision(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        revision_id: Uuid,
    ) -> Result<notegate_model::text_revision::TextRevisionContent> {
        crate::files::revisions::read(
            &self.pool,
            &self.crypto,
            space_id,
            node_id,
            revision_id,
            self.external_only,
        )
        .await
    }
}
