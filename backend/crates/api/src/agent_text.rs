use notegate_model::TextStorageFormat;
use notegate_service::files::{ReadText, ReadTextBody};
use notegate_service::{ServiceError, ServiceResult};
use uuid::Uuid;

use crate::state::AppState;

pub async fn guarded_plain_text_sha(
    state: &AppState,
    account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    expected_sha256: Option<&str>,
) -> ServiceResult<String> {
    let result = state
        .files
        .read_text(
            account_id,
            space_id,
            ReadText {
                node_id,
                start_line: None,
                max_lines: None,
                max_bytes: Some(1),
                if_none_match_sha256: None,
            },
        )
        .await?;
    if result.storage_format == TextStorageFormat::Encrypted
        || matches!(&result.body, ReadTextBody::Encrypted(_))
    {
        return Err(ServiceError::InvalidInput(
            "encrypted text cannot be modified through Agent content APIs".to_owned(),
        ));
    }
    if expected_sha256.is_some_and(|expected| expected != result.content_sha256) {
        return Err(ServiceError::Conflict(
            "expected_sha256 does not match the current text; read it again".to_owned(),
        ));
    }
    Ok(result.content_sha256)
}
