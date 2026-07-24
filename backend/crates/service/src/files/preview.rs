use std::collections::HashSet;

use notegate_model::{FileObject, Node, NodeKind};
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};
use crate::files::{FileCommand, validation};

use super::FilesService;

pub const MAX_BATCH_PREVIEW_PATHS: usize = 64;
pub const MAX_BATCH_PREVIEW_PATH_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct BatchPreviewCandidate {
    pub path: String,
    pub node: Option<Node>,
    pub file: Option<FileObject>,
}

impl FilesService {
    /// Resolve an ordered set of preview paths with one authorization check and
    /// a constant number of database queries.
    pub async fn batch_preview_candidates(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        paths: Vec<String>,
    ) -> ServiceResult<Vec<BatchPreviewCandidate>> {
        self.authorize(space_id, caller_account_id, FileCommand::Read)
            .await?;
        let paths = normalize_batch_paths(paths)?;
        let resolved = self.store.resolve_nodes_by_paths(space_id, &paths).await?;

        let mut nodes = vec![None; paths.len()];
        for (index, _path, node) in resolved {
            let target = nodes
                .get_mut(index)
                .ok_or_else(|| ServiceError::Internal("invalid batch path index".to_owned()))?;
            *target = Some(node);
        }

        let file_ids = nodes
            .iter()
            .flatten()
            .filter(|node| node.kind == NodeKind::File)
            .map(|node| node.id)
            .collect::<Vec<_>>();
        let mut files = self.store.find_files(space_id, &file_ids).await?;

        Ok(paths
            .into_iter()
            .zip(nodes)
            .map(|(path, node)| {
                let file = node.as_ref().and_then(|node| files.remove(&node.id));
                BatchPreviewCandidate { path, node, file }
            })
            .collect())
    }
}

fn normalize_batch_paths(paths: Vec<String>) -> ServiceResult<Vec<String>> {
    if paths.is_empty() {
        return Err(ServiceError::InvalidInput(
            "paths must contain at least one item".to_owned(),
        ));
    }
    if paths.len() > MAX_BATCH_PREVIEW_PATHS {
        return Err(ServiceError::InvalidInput(format!(
            "paths must contain at most {MAX_BATCH_PREVIEW_PATHS} items"
        )));
    }

    let mut normalized = Vec::with_capacity(paths.len());
    let mut seen = HashSet::with_capacity(paths.len());
    let mut total_bytes = 0usize;
    for path in paths {
        let path = validation::normalize_path(&path)?;
        total_bytes = total_bytes.saturating_add(path.len());
        if total_bytes > MAX_BATCH_PREVIEW_PATH_BYTES {
            return Err(ServiceError::InvalidInput(format!(
                "paths must contain at most {MAX_BATCH_PREVIEW_PATH_BYTES} bytes"
            )));
        }
        if !seen.insert(path.clone()) {
            return Err(ServiceError::InvalidInput(
                "paths must be unique after normalization".to_owned(),
            ));
        }
        normalized.push(path);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_paths_reject_duplicates_after_normalization() {
        let result =
            normalize_batch_paths(vec!["/images/logo.png".into(), "//images/logo.png".into()]);
        assert!(matches!(result, Err(ServiceError::InvalidInput(_))));
    }

    #[test]
    fn batch_paths_enforce_count_and_byte_bounds() {
        let too_many = vec!["/a.png".to_owned(); MAX_BATCH_PREVIEW_PATHS + 1];
        assert!(normalize_batch_paths(too_many).is_err());
        assert!(
            normalize_batch_paths(vec![format!(
                "/{}.png",
                "a".repeat(MAX_BATCH_PREVIEW_PATH_BYTES)
            )])
            .is_err()
        );
    }
}
