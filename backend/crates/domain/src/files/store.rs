use std::future::Future;

use uuid::Uuid;

use super::{
    Children, CreateDocument, CreateFolder, DocumentBundle, FilesResult, FindQuery, GrepCandidate,
    GrepCandidateQuery, MoveNode, Node, SaveDocument,
};

pub trait FilesStore: Clone + Send + Sync + 'static {
    fn initialize_root_node(&self, user_id: Uuid)
    -> impl Future<Output = FilesResult<Node>> + Send;

    fn resolve_node(
        &self,
        user_id: Uuid,
        path: String,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn child_nodes(
        &self,
        user_id: Uuid,
        parent_node_id: Uuid,
    ) -> impl Future<Output = FilesResult<Children>> + Send;

    fn create_folder(
        &self,
        user_id: Uuid,
        command: CreateFolder,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn create_document(
        &self,
        user_id: Uuid,
        command: CreateDocument,
    ) -> impl Future<Output = FilesResult<DocumentBundle>> + Send;

    fn document(
        &self,
        user_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = FilesResult<DocumentBundle>> + Send;

    fn save_document(
        &self,
        user_id: Uuid,
        command: SaveDocument,
    ) -> impl Future<Output = FilesResult<DocumentBundle>> + Send;

    fn move_node(
        &self,
        user_id: Uuid,
        command: MoveNode,
    ) -> impl Future<Output = FilesResult<Node>> + Send;

    fn delete_node(
        &self,
        user_id: Uuid,
        node_id: Uuid,
    ) -> impl Future<Output = FilesResult<()>> + Send;

    fn find_nodes(
        &self,
        user_id: Uuid,
        query: FindQuery,
    ) -> impl Future<Output = FilesResult<Vec<Node>>> + Send;

    fn grep_candidates(
        &self,
        user_id: Uuid,
        query: GrepCandidateQuery,
    ) -> impl Future<Output = FilesResult<Vec<GrepCandidate>>> + Send;
}
