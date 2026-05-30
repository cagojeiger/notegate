use sqlx::PgPool;

mod document;
mod error;
mod node;
mod rows;
mod search;
mod store;
#[cfg(test)]
mod tests;
mod workspace;

pub use notegate_domain::files::{
    Children, CreateDocument, CreateFolder, Document, DocumentBundle, FilesError, FilesResult,
    FilesService, FindRequest, GrepMatch, GrepRequest, MoveNode, Node, NodeKind, SaveDocument,
};

#[derive(Debug, Clone)]
pub struct FilesRepo {
    pool: PgPool,
}

impl FilesRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(super) fn pool(&self) -> &PgPool {
        &self.pool
    }
}
