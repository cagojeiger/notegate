//! Pure match preparation and evaluation for `find` and `grep`.

use notegate_core::limits;
use notegate_model::search::{FindMatchMode, GrepLineMode, GrepMatchMode};
use regex::{Regex, RegexBuilder};

use crate::error::{ServiceError, ServiceResult};

pub(super) enum NameMatcher {
    Contains(String),
    Regex(Regex),
    Glob(Regex),
}

impl NameMatcher {
    pub(super) fn new(q: &str, mode: FindMatchMode) -> ServiceResult<Self> {
        match mode {
            FindMatchMode::Contains => Ok(Self::Contains(q.to_lowercase())),
            FindMatchMode::Regex => Ok(Self::Regex(compile_regex(q)?)),
            FindMatchMode::Glob => Ok(Self::Glob(compile_glob(q)?)),
        }
    }

    pub(super) fn is_match(&self, value: &str) -> bool {
        match self {
            Self::Contains(needle) => value.to_lowercase().contains(needle),
            Self::Regex(regex) | Self::Glob(regex) => regex.is_match(value),
        }
    }
}

pub(super) struct ContentMatcher {
    regex: Regex,
}

impl ContentMatcher {
    pub(super) fn new(q: &str, mode: GrepMatchMode) -> ServiceResult<Self> {
        let regex = match mode {
            GrepMatchMode::Literal => compile_regex(&regex::escape(q))?,
            GrepMatchMode::Regex => compile_regex(q)?,
        };
        Ok(Self { regex })
    }

    pub(super) fn match_lines(&self, content: &str, mode: GrepLineMode) -> Vec<i32> {
        if content.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::new();
        for (index, line) in logical_lines(content).enumerate() {
            if !self.regex.is_match(line) {
                continue;
            }

            let line_number = index as i32 + 1;
            match mode {
                GrepLineMode::None => return vec![line_number],
                GrepLineMode::First => return vec![line_number],
                GrepLineMode::All => lines.push(line_number),
            }
        }
        lines
    }
}

pub(super) fn logical_lines(content: &str) -> impl Iterator<Item = &str> {
    let content = content.strip_suffix('\n').unwrap_or(content);
    content.split('\n')
}

pub(super) struct PathFilters {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

impl PathFilters {
    pub(super) fn new(include: &[String], exclude: &[String]) -> ServiceResult<Self> {
        validate_glob_patterns("include", include)?;
        validate_glob_patterns("exclude", exclude)?;
        Ok(Self {
            include: include
                .iter()
                .map(|pattern| compile_glob(pattern))
                .collect::<ServiceResult<_>>()?,
            exclude: exclude
                .iter()
                .map(|pattern| compile_glob(pattern))
                .collect::<ServiceResult<_>>()?,
        })
    }

    pub(super) fn allows(&self, path: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|regex| regex.is_match(path)))
            && !self.exclude.iter().any(|regex| regex.is_match(path))
    }
}

fn validate_glob_patterns(label: &str, patterns: &[String]) -> ServiceResult<()> {
    if patterns.len() > limits::SEARCH_GLOB_PATTERNS_MAX {
        return Err(ServiceError::InvalidInput(format!(
            "{label} must contain at most {} glob patterns",
            limits::SEARCH_GLOB_PATTERNS_MAX
        )));
    }
    for pattern in patterns {
        if pattern.chars().count() > limits::SEARCH_GLOB_PATTERN_MAX_CHARS {
            return Err(ServiceError::InvalidInput(format!(
                "{label} glob patterns must be at most {} characters",
                limits::SEARCH_GLOB_PATTERN_MAX_CHARS
            )));
        }
    }
    Ok(())
}

fn compile_regex(pattern: &str) -> ServiceResult<Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|error| ServiceError::InvalidInput(format!("invalid regex pattern: {error}")))
}

fn compile_glob(pattern: &str) -> ServiceResult<Regex> {
    let mut out = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            _ => out.push_str(&regex::escape(&ch.to_string())),
        }
    }
    out.push('$');
    compile_regex(&out)
}
