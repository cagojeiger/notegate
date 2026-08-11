use notegate_model::FileEncryptionMode;
use serde::Serialize;
use utoipa::ToSchema;

use crate::object_storage::{ObjectStorage, ObjectStorageError};

pub const PREVIEW_URL_TTL_SECONDS: i64 = 15 * 60;
pub const PREVIEW_MAX_BYTES: i64 = 10 * 1024 * 1024;
const PDF_MEDIA_TYPE: &str = "application/pdf";
const MEDIA_TYPE_SNIFF_BYTES: usize = 8 * 1024;
const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilePreviewKind {
    Image,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileMediaKind {
    Image,
    Pdf,
    Audio,
    Other,
}

pub async fn detect_object_media_type(
    storage: &ObjectStorage,
    object_key: &str,
    byte_len: i64,
    encryption_mode: FileEncryptionMode,
) -> Result<Option<String>, ObjectStorageError> {
    if encryption_mode == FileEncryptionMode::Client {
        return Ok(None);
    }
    if byte_len == 0 {
        return Ok(Some(UNKNOWN_MEDIA_TYPE.to_owned()));
    }

    let bytes = storage
        .read_prefix(object_key, MEDIA_TYPE_SNIFF_BYTES)
        .await?;
    let media_type = infer::get(&bytes)
        .map(|kind| kind.mime_type())
        .unwrap_or(UNKNOWN_MEDIA_TYPE);
    Ok(Some(media_type.to_owned()))
}

pub fn is_preview_size_allowed(byte_len: i64) -> bool {
    byte_len > 0 && byte_len <= PREVIEW_MAX_BYTES
}

pub fn is_previewable_image_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/avif" | "image/gif"
    )
}

pub fn is_previewable_pdf_type(media_type: &str) -> bool {
    media_type == PDF_MEDIA_TYPE
}

pub fn file_preview_kind(
    byte_len: i64,
    encryption_mode: FileEncryptionMode,
    detected_media_type: Option<&str>,
) -> Option<FilePreviewKind> {
    if encryption_mode != FileEncryptionMode::None || !is_preview_size_allowed(byte_len) {
        return None;
    }

    match detected_media_type {
        Some(media_type) if is_previewable_image_type(media_type) => Some(FilePreviewKind::Image),
        Some(media_type) if is_previewable_pdf_type(media_type) => Some(FilePreviewKind::Pdf),
        _ => None,
    }
}

pub fn file_media_kind(
    declared_media_type: &str,
    detected_media_type: Option<&str>,
) -> FileMediaKind {
    match detected_media_type {
        Some(media_type) if media_type.starts_with("image/") => FileMediaKind::Image,
        Some(media_type) if is_previewable_pdf_type(media_type) => FileMediaKind::Pdf,
        Some(media_type) if media_type.starts_with("audio/") => FileMediaKind::Audio,
        Some("video/mp4" | "video/webm") if declared_media_type.starts_with("audio/") => {
            FileMediaKind::Audio
        }
        None if declared_media_type.starts_with("audio/") => FileMediaKind::Audio,
        _ => FileMediaKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use notegate_model::FileEncryptionMode;

    use super::{
        FileMediaKind, FilePreviewKind, PREVIEW_MAX_BYTES, file_media_kind, file_preview_kind,
        is_preview_size_allowed, is_previewable_image_type, is_previewable_pdf_type,
    };

    #[test]
    fn only_safe_raster_image_types_are_previewable() {
        for media_type in [
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/avif",
            "image/gif",
        ] {
            assert!(is_previewable_image_type(media_type), "{media_type}");
        }
        for media_type in [
            "image/svg+xml",
            "application/pdf",
            "text/html",
            "application/octet-stream",
        ] {
            assert!(!is_previewable_image_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn only_pdf_type_is_previewable_pdf() {
        assert!(is_previewable_pdf_type("application/pdf"));
        for media_type in ["image/png", "text/html", "application/octet-stream"] {
            assert!(!is_previewable_pdf_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn preview_size_is_limited_to_ten_mib() {
        assert!(!is_preview_size_allowed(0));
        assert!(is_preview_size_allowed(PREVIEW_MAX_BYTES));
        assert!(!is_preview_size_allowed(PREVIEW_MAX_BYTES + 1));
    }

    #[test]
    fn file_preview_kind_uses_verified_media_type_and_file_policy() {
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("image/png")),
            Some(FilePreviewKind::Image)
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("application/pdf")),
            Some(FilePreviewKind::Pdf)
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::Client, Some("application/pdf")),
            None
        );
        assert_eq!(
            file_preview_kind(
                PREVIEW_MAX_BYTES + 1,
                FileEncryptionMode::None,
                Some("application/pdf")
            ),
            None
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, Some("text/html")),
            None
        );
        assert_eq!(
            file_preview_kind(1024, FileEncryptionMode::None, None),
            None
        );
    }

    #[test]
    fn file_media_kind_prefers_detection_and_recognizes_browser_audio_containers() {
        assert_eq!(
            file_media_kind("application/octet-stream", Some("audio/mpeg")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("audio/mp4", Some("video/mp4")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("audio/webm;codecs=opus", Some("video/webm")),
            FileMediaKind::Audio
        );
        assert_eq!(
            file_media_kind("video/mp4", Some("video/mp4")),
            FileMediaKind::Other
        );
        assert_eq!(
            file_media_kind("audio/mp4", Some("text/html")),
            FileMediaKind::Other
        );
        assert_eq!(
            file_media_kind("image/png", Some("image/png")),
            FileMediaKind::Image
        );
        assert_eq!(
            file_media_kind("application/pdf", Some("application/pdf")),
            FileMediaKind::Pdf
        );
        assert_eq!(file_media_kind("audio/mp4", None), FileMediaKind::Audio);
    }
}
