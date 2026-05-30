use super::{FilesError, FilesResult};

pub(super) fn validate_folder_name(name: &str) -> FilesResult<()> {
    validate_base_name(name)?;
    if name.ends_with(".md") {
        return Err(FilesError::InvalidInput(
            "folder name cannot end with .md".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_document_name(name: &str) -> FilesResult<()> {
    validate_base_name(name)?;
    if !name.ends_with(".md") {
        return Err(FilesError::InvalidInput(
            "document name must end with .md".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_node_name(name: &str) -> FilesResult<()> {
    validate_base_name(name)
}

fn validate_base_name(name: &str) -> FilesResult<()> {
    if name.is_empty() {
        return Err(FilesError::InvalidInput("name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(FilesError::InvalidInput("invalid name".into()));
    }
    if name.contains('/') {
        return Err(FilesError::InvalidInput("name cannot contain /".into()));
    }
    Ok(())
}

pub(super) fn normalize_path(path: &str) -> FilesResult<String> {
    if !path.starts_with('/') {
        return Err(FilesError::InvalidInput("path must start with /".into()));
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "." || segment == ".." {
            return Err(FilesError::InvalidInput(
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

pub(super) fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 100)
}
