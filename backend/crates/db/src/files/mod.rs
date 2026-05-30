use sqlx::PgPool;

mod commands;
mod error;
mod queries;
mod rows;
mod store;
#[cfg(test)]
mod tests;

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
