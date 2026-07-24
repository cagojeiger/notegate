use notegate_model::FileEncryptionMode;

use crate::object_storage::{ObjectStorage, ObjectStorageError};

pub const PREVIEW_URL_TTL_SECONDS: i64 = 15 * 60;
pub const PREVIEW_MAX_BYTES: i64 = 10 * 1024 * 1024;
const MEDIA_TYPE_SNIFF_BYTES: usize = 8 * 1024;
const UNKNOWN_MEDIA_TYPE: &str = "application/octet-stream";

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

pub fn is_previewable_media_type(media_type: &str) -> bool {
    is_previewable_image_type(media_type) || media_type == "application/pdf"
}

#[cfg(test)]
mod tests {
    use super::{
        PREVIEW_MAX_BYTES, is_preview_size_allowed, is_previewable_image_type,
        is_previewable_media_type,
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
    fn safe_raster_images_and_pdf_are_previewable_media() {
        for media_type in [
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/avif",
            "image/gif",
            "application/pdf",
        ] {
            assert!(is_previewable_media_type(media_type), "{media_type}");
        }
        for media_type in ["image/svg+xml", "text/html", "application/octet-stream"] {
            assert!(!is_previewable_media_type(media_type), "{media_type}");
        }
    }

    #[test]
    fn preview_size_is_limited_to_ten_mib() {
        assert!(!is_preview_size_allowed(0));
        assert!(is_preview_size_allowed(PREVIEW_MAX_BYTES));
        assert!(!is_preview_size_allowed(PREVIEW_MAX_BYTES + 1));
    }
}
