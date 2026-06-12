//! Syntax validation for structured plain-text files.
//!
//! Validation is intentionally syntax-only. It prevents obviously broken JSON,
//! JSONL, YAML, and TOML from being persisted after a text mutation, without
//! introducing schema-specific product rules.

use crate::error::{ServiceError, ServiceResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuredFormat {
    Json,
    Jsonl,
    Yaml,
    Toml,
}

impl StructuredFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
        }
    }
}

/// Validate syntax for well-known structured text file names.
///
/// Unknown extensions are treated as free-form text.
pub fn validate_structured_text(name: &str, content: &str) -> ServiceResult<()> {
    let Some(format) = infer_format(name) else {
        return Ok(());
    };

    match format {
        StructuredFormat::Json => validate_json(name, content),
        StructuredFormat::Jsonl => validate_jsonl(name, content),
        StructuredFormat::Yaml => validate_yaml(name, content),
        StructuredFormat::Toml => validate_toml(name, content),
    }
}

fn infer_format(name: &str) -> Option<StructuredFormat> {
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "json" => Some(StructuredFormat::Json),
        "jsonl" => Some(StructuredFormat::Jsonl),
        "yaml" | "yml" => Some(StructuredFormat::Yaml),
        "toml" => Some(StructuredFormat::Toml),
        _ => None,
    }
}

fn validate_json(name: &str, content: &str) -> ServiceResult<()> {
    serde_json::from_str::<serde_json::Value>(content).map_err(|error| {
        invalid_format(
            StructuredFormat::Json,
            name,
            Some(error.line()),
            Some(error.column()),
            error.to_string(),
        )
    })?;
    Ok(())
}

fn validate_jsonl(name: &str, content: &str) -> ServiceResult<()> {
    if content.is_empty() {
        return Err(invalid_format(
            StructuredFormat::Jsonl,
            name,
            Some(1),
            Some(1),
            "jsonl content must contain at least one JSON line".to_owned(),
        ));
    }

    for (index, line) in content.lines().enumerate() {
        let line_no = index + 1;
        if line.trim().is_empty() {
            return Err(invalid_format(
                StructuredFormat::Jsonl,
                name,
                Some(line_no),
                Some(1),
                "blank lines are not valid JSONL records".to_owned(),
            ));
        }
        serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
            invalid_format(
                StructuredFormat::Jsonl,
                name,
                Some(line_no),
                Some(error.column()),
                error.to_string(),
            )
        })?;
    }
    Ok(())
}

fn validate_yaml(name: &str, content: &str) -> ServiceResult<()> {
    yaml_rust2::YamlLoader::load_from_str(content).map_err(|error| {
        let marker = error.marker();
        invalid_format(
            StructuredFormat::Yaml,
            name,
            Some(marker.line()),
            Some(marker.col()),
            error.to_string(),
        )
    })?;
    Ok(())
}

fn validate_toml(name: &str, content: &str) -> ServiceResult<()> {
    toml::from_str::<toml::Value>(content).map_err(|error| {
        let (line, column) = error
            .span()
            .map(|span| line_column(content, span.start))
            .unwrap_or((None, None));
        invalid_format(
            StructuredFormat::Toml,
            name,
            line,
            column,
            error.to_string(),
        )
    })?;
    Ok(())
}

fn line_column(content: &str, byte_offset: usize) -> (Option<usize>, Option<usize>) {
    let offset = byte_offset.min(content.len());
    let prefix = &content[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_before, after)| after)
        .chars()
        .count()
        + 1;
    (Some(line), Some(column))
}

fn invalid_format(
    format: StructuredFormat,
    name: &str,
    line: Option<usize>,
    column: Option<usize>,
    detail: String,
) -> ServiceError {
    let location = match (line, column) {
        (Some(line), Some(column)) => format!(" at line {line}, column {column}"),
        (Some(line), None) => format!(" at line {line}"),
        _ => String::new(),
    };
    ServiceError::InvalidInput(format!(
        "invalid {} syntax in {name}{location}: {detail}",
        format.label()
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn invalid(name: &str, content: &str) -> String {
        match validate_structured_text(name, content).unwrap_err() {
            ServiceError::InvalidInput(message) => message,
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn unknown_extensions_are_free_form() {
        validate_structured_text("note.md", "{not json").unwrap();
    }

    #[test]
    fn validates_json() {
        validate_structured_text("config.json", r#"{"ok":true}"#).unwrap();
        let message = invalid("config.json", r#"{"ok":}"#);
        assert!(message.contains("invalid json syntax in config.json"));
        assert!(message.contains("line 1"));
    }

    #[test]
    fn validates_jsonl_per_line() {
        validate_structured_text("events.jsonl", "{\"a\":1}\n[2]\n").unwrap();
        let message = invalid("events.jsonl", "{\"a\":1}\n\n");
        assert!(message.contains("blank lines are not valid JSONL records"));
        assert!(message.contains("line 2"));
    }

    #[test]
    fn validates_yaml_and_toml() {
        validate_structured_text("config.yaml", "a:\n  b: 1\n").unwrap();
        validate_structured_text("config.yml", "a: [1, 2]\n").unwrap();
        validate_structured_text("config.toml", "[a]\nb = 1\n").unwrap();
        assert!(invalid("config.toml", "[a\nb = 1").contains("invalid toml syntax"));
    }
}
