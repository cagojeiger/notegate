//! Read queries for the file tree: node metadata + path derivation, document
//! content/metrics, and search (`find`/`grep`) with recursive-CTE scope.

pub mod document;
pub mod node;
pub mod search;
