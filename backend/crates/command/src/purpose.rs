use std::fmt;

pub const PURPOSE_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurposeValidationError {
    EmptyOrPadded,
    TooLong,
}

impl fmt::Display for PurposeValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyOrPadded => formatter.write_str(
                "purpose must be non-empty and must not have leading or trailing whitespace",
            ),
            Self::TooLong => write!(
                formatter,
                "purpose must be at most {PURPOSE_MAX_CHARS} characters"
            ),
        }
    }
}

/// Validate the shared command-purpose contract.
pub fn validate_purpose(purpose: &str) -> Result<(), PurposeValidationError> {
    let char_count = purpose.chars().count();
    if char_count == 0 || purpose.trim() != purpose {
        return Err(PurposeValidationError::EmptyOrPadded);
    }
    if char_count > PURPOSE_MAX_CHARS {
        return Err(PurposeValidationError::TooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_or_padded_purpose() {
        assert_eq!(
            validate_purpose(""),
            Err(PurposeValidationError::EmptyOrPadded)
        );
        assert_eq!(
            PurposeValidationError::EmptyOrPadded.to_string(),
            "purpose must be non-empty and must not have leading or trailing whitespace"
        );
        assert_eq!(
            validate_purpose(" search daily notes "),
            Err(PurposeValidationError::EmptyOrPadded)
        );
    }

    #[test]
    fn rejects_purpose_over_the_unicode_character_limit() {
        assert_eq!(
            validate_purpose(&"가".repeat(PURPOSE_MAX_CHARS + 1)),
            Err(PurposeValidationError::TooLong)
        );
        assert_eq!(
            PurposeValidationError::TooLong.to_string(),
            "purpose must be at most 200 characters"
        );
    }

    #[test]
    fn accepts_a_bounded_unicode_purpose() {
        assert!(validate_purpose("오늘 변경된 검색 설계 노트를 확인").is_ok());
        assert!(validate_purpose(&"가".repeat(PURPOSE_MAX_CHARS)).is_ok());
    }
}
