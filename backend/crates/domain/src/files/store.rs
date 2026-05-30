use std::future::Future;

use uuid::Uuid;

use super::{DocumentBundle, FilesResult, FindQuery, GrepCandidate, GrepCandidateQuery, Node};

pub trait FilesStore: Clone + Send + Sync + 'static {
    fn initialize_default_workspace(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = FilesResult<Uuid>> + Send;

    fn default_workspace_id(&self, user_id: Uuid)
    -> impl Future<Output = FilesResult<Uuid>> + Send;

    fn root_for_workspace(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn node_by_id(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn node_by_path(
        &self,
        workspace_id: Uuid,
        path: &str,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn child_nodes(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
    ) -> impl Future<Output = FilesResult<Vec<Node>>> + Send;

    fn create_folder_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn create_document_node(
        &self,
        workspace_id: Uuid,
        parent_node_id: Uuid,
        name: &str,
        path: &str,
    ) -> impl Future<Output = FilesResult<DocumentBundle>> + Send;

    fn document_by_node_id(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = FilesResult<DocumentBundle>> + Send;

    fn save_document_content(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        content_md: &str,
    ) -> impl Future<Output = FilesResult<()>> + Send;

    fn move_node_record(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
        new_parent_node_id: Uuid,
        new_name: &str,
        old_path: &str,
        new_path: &str,
    ) -> impl Future<Output = FilesResult<()>> + Send;

    fn soft_delete_subtree(
        &self,
        workspace_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = FilesResult<()>> + Send;

    fn find_nodes(
        &self,
        workspace_id: Uuid,
        query: FindQuery,
    ) -> impl Future<Output = FilesResult<Vec<Node>>> + Send;

    fn grep_candidates(
        &self,
        workspace_id: Uuid,
        query: GrepCandidateQuery,
    ) -> impl Future<Output = FilesResult<Vec<GrepCandidate>>> + Send;
}
