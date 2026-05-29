use super::error::{VaultRepoError, VaultResult};

pub(super) fn validate_folder_name(name: &str) -> VaultResult<()> {
    validate_base_name(name)?;
    if name.ends_with(".md") {
        return Err(VaultRepoError::InvalidInput(
            "folder name cannot end with .md".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_document_name(name: &str) -> VaultResult<()> {
    validate_base_name(name)?;
    if !name.ends_with(".md") {
        return Err(VaultRepoError::InvalidInput(
            "document name must end with .md".into(),
        ));
    }
    Ok(())
}

fn validate_base_name(name: &str) -> VaultResult<()> {
    if name.is_empty() {
        return Err(VaultRepoError::InvalidInput("name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(VaultRepoError::InvalidInput("invalid name".into()));
    }
    if name.contains('/') {
        return Err(VaultRepoError::InvalidInput("name cannot contain /".into()));
    }
    Ok(())
}

pub(super) fn normalize_path(path: &str) -> VaultResult<String> {
    if !path.starts_with('/') {
        return Err(VaultRepoError::InvalidInput(
            "path must start with /".into(),
        ));
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "." || segment == ".." {
            return Err(VaultRepoError::InvalidInput(
                "path cannot contain . or ..".into(),
            ));
        }
        segments.push(segment);
    }

    if segments.is_empty() {
        Ok("/".into())
    } else {
        Ok(format!("/{}", segments.join("/")))
    }
}

pub(super) fn child_path(parent_path: &str, name: &str) -> String {
    if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{parent_path}/{name}")
    }
}

pub(super) fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 100)
}
