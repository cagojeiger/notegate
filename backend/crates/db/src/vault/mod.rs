use sqlx::PgPool;

mod document;
mod error;
mod model;
mod node;
mod rows;
mod search;
#[cfg(test)]
mod tests;
mod validation;
mod workspace;

pub use error::{VaultRepoError, VaultResult};
pub use model::{
    Children, Document, DocumentBundle, FindRequest, GrepMatch, GrepRequest, Node, NodeKind,
};

#[derive(Debug, Clone)]
pub struct VaultRepo {
    pool: PgPool,
}

impl VaultRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(super) fn pool(&self) -> &PgPool {
        &self.pool
    }
}
