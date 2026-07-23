//! S3-compatible object storage access and presigned transfer URLs.

use std::collections::BTreeMap;
use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_smithy_http_client::hyper_014::HyperClientBuilder;
use notegate_core::S3Config;
use notegate_core::limits::SINGLE_PUT_MAX_BYTES;
use secrecy::ExposeSecret as _;
use tokio::io::AsyncReadExt as _;

use crate::error::ApiError;

pub const TRANSFER_URL_TTL: Duration = Duration::from_secs(15 * 60);
pub const MCP_TRANSFER_URL_TTL: Duration = Duration::from_secs(5 * 60);
pub const MULTIPART_PART_SIZE: i64 = 64 * 1024 * 1024;
const MAX_MULTIPART_PARTS: i32 = 10_000;

pub struct PresignedPut {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedUploadPart {
    pub part_number: i32,
    pub etag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStorageError {
    Missing,
    SizeMismatch,
    InvalidMultipart,
    Unavailable,
}

impl From<ObjectStorageError> for ApiError {
    fn from(error: ObjectStorageError) -> Self {
        match error {
            ObjectStorageError::Missing => ApiError::conflict(
                "uploaded object was not found; upload the file before completing",
            ),
            ObjectStorageError::SizeMismatch => {
                ApiError::invalid_field("uploaded object size does not match the declared size")
            }
            ObjectStorageError::InvalidMultipart => {
                ApiError::invalid_field("multipart completion parts are invalid")
            }
            ObjectStorageError::Unavailable => ApiError::object_storage_unavailable(),
        }
    }
}

#[derive(Clone)]
pub struct ObjectStorage {
    internal: aws_sdk_s3::Client,
    public: aws_sdk_s3::Client,
    bucket: String,
}

impl ObjectStorage {
    pub fn new(config: &S3Config) -> Self {
        let public_endpoint = config
            .public_endpoint
            .as_deref()
            .unwrap_or(&config.endpoint);
        Self {
            internal: client(config, &config.endpoint),
            public: client(config, public_endpoint),
            bucket: config.bucket.clone(),
        }
    }

    pub async fn presign_put_with_ttl(
        &self,
        object_key: &str,
        content_type: &str,
        content_length: i64,
        ttl: Duration,
    ) -> Result<PresignedPut, ObjectStorageError> {
        let presigned = self
            .public
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .content_type(content_type)
            .content_length(content_length)
            .if_none_match("*")
            .presigned(presigning_config(ttl)?)
            .await
            .map_err(|error| unavailable("presign_put", error))?;
        Ok(PresignedPut {
            url: presigned.uri().to_owned(),
            headers: presigned
                .headers()
                // Browsers generate Content-Length from the request body and
                // forbid JavaScript from setting it. It remains part of the
                // signature, but is not returned as a caller-supplied header.
                .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        })
    }

    pub async fn create_multipart_upload(
        &self,
        object_key: &str,
        content_type: &str,
    ) -> Result<String, ObjectStorageError> {
        let result = self
            .internal
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(object_key)
            .content_type(content_type)
            .send()
            .await
            .map_err(|error| unavailable("create_multipart_upload", error))?;
        result.upload_id().map(str::to_owned).ok_or_else(|| {
            tracing::error!(event = "object_storage.multipart_upload_id_missing");
            ObjectStorageError::Unavailable
        })
    }

    pub async fn presign_upload_part(
        &self,
        object_key: &str,
        upload_id: &str,
        part_number: i32,
        content_length: i64,
        ttl: Duration,
    ) -> Result<PresignedPut, ObjectStorageError> {
        let presigned = self
            .public
            .upload_part()
            .bucket(&self.bucket)
            .key(object_key)
            .upload_id(upload_id)
            .part_number(part_number)
            .content_length(content_length)
            .presigned(presigning_config(ttl)?)
            .await
            .map_err(|error| unavailable("presign_upload_part", error))?;
        Ok(PresignedPut {
            url: presigned.uri().to_owned(),
            headers: presigned
                .headers()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        })
    }

    pub async fn complete_multipart_upload(
        &self,
        object_key: &str,
        upload_id: &str,
        parts: &[CompletedUploadPart],
    ) -> Result<(), ObjectStorageError> {
        let mut multipart = CompletedMultipartUpload::builder();
        for part in parts {
            multipart = multipart.parts(
                CompletedPart::builder()
                    .part_number(part.part_number)
                    .e_tag(part.etag.clone())
                    .build(),
            );
        }
        let result = self
            .internal
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(object_key)
            .upload_id(upload_id)
            .multipart_upload(multipart.build())
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error) => {
                let code = error
                    .as_service_error()
                    .and_then(|error| error.meta().code());
                match code {
                    Some("InvalidPart" | "InvalidPartOrder" | "EntityTooSmall") => {
                        Err(ObjectStorageError::InvalidMultipart)
                    }
                    Some("NoSuchUpload") => Err(ObjectStorageError::Missing),
                    _ => Err(unavailable("complete_multipart_upload", error)),
                }
            }
        }
    }

    pub async fn abort_multipart_upload(
        &self,
        object_key: &str,
        upload_id: &str,
    ) -> Result<(), ObjectStorageError> {
        let result = self
            .internal
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(object_key)
            .upload_id(upload_id)
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_no_such_upload()) =>
            {
                Ok(())
            }
            Err(error) => Err(unavailable("abort_multipart_upload", error)),
        }
    }

    pub async fn verify_upload(
        &self,
        object_key: &str,
        expected_size: i64,
    ) -> Result<String, ObjectStorageError> {
        let result = self
            .internal
            .head_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await;
        let head = match result {
            Ok(head) => head,
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_not_found()) =>
            {
                return Err(ObjectStorageError::Missing);
            }
            Err(error) => return Err(unavailable("head_object", error)),
        };
        if head.content_length() != Some(expected_size) {
            return Err(ObjectStorageError::SizeMismatch);
        }
        Ok(head
            .e_tag()
            .unwrap_or_default()
            .trim_matches('"')
            .to_owned())
    }

    pub async fn presign_get(
        &self,
        object_key: &str,
        filename: Option<&str>,
    ) -> Result<String, ObjectStorageError> {
        self.presign_get_with_ttl(object_key, filename, TRANSFER_URL_TTL)
            .await
    }

    pub async fn presign_get_with_ttl(
        &self,
        object_key: &str,
        filename: Option<&str>,
        ttl: Duration,
    ) -> Result<String, ObjectStorageError> {
        // Always force a download disposition: object media types are
        // client-declared, so an inline render on the storage origin would let a
        // caller serve `text/html` from a trusted bucket domain.
        let disposition = match filename {
            Some(filename) => format!("attachment; filename*=UTF-8''{}", rfc5987_encode(filename)),
            None => "attachment".to_owned(),
        };
        let presigned = self
            .public
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .response_content_disposition(disposition)
            .presigned(presigning_config(ttl)?)
            .await
            .map_err(|error| unavailable("presign_get", error))?;
        Ok(presigned.uri().to_owned())
    }

    pub async fn presign_inline_get(
        &self,
        object_key: &str,
        media_type: &str,
        ttl: Duration,
    ) -> Result<String, ObjectStorageError> {
        let presigned = self
            .public
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .response_cache_control("private, no-store, max-age=0")
            .response_content_disposition("inline")
            .response_content_type(media_type)
            .presigned(presigning_config(ttl)?)
            .await
            .map_err(|error| unavailable("presign_inline_get", error))?;
        Ok(presigned.uri().to_owned())
    }

    pub async fn read_prefix(
        &self,
        object_key: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, ObjectStorageError> {
        let end = max_bytes
            .checked_sub(1)
            .ok_or(ObjectStorageError::Unavailable)?;
        let output = self
            .internal
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .range(format!("bytes=0-{end}"))
            .send()
            .await
            .map_err(|error| unavailable("read_prefix", error))?;
        if output
            .content_length()
            .is_some_and(|length| length > max_bytes as i64)
        {
            tracing::error!(
                event = "object_storage.range_ignored",
                object_key,
                max_bytes,
            );
            return Err(ObjectStorageError::Unavailable);
        }
        let read_limit = u64::try_from(max_bytes)
            .map_err(|_| ObjectStorageError::Unavailable)?
            .saturating_add(1);
        let mut bytes = Vec::with_capacity(max_bytes.min(8 * 1024));
        output
            .body
            .into_async_read()
            .take(read_limit)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| unavailable("read_prefix_body", error))?;
        if bytes.len() > max_bytes {
            return Err(ObjectStorageError::Unavailable);
        }
        Ok(bytes)
    }

    pub async fn delete(&self, object_key: &str) -> Result<(), ObjectStorageError> {
        self.internal
            .delete_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(|error| unavailable("delete_object", error))?;
        Ok(())
    }
}

pub fn uses_multipart(byte_len: i64) -> bool {
    byte_len > SINGLE_PUT_MAX_BYTES as i64
}

pub fn multipart_part_count(byte_len: i64, part_size: i64) -> Option<i32> {
    if byte_len <= 0 || part_size <= 0 {
        return None;
    }
    let count = byte_len
        .checked_add(part_size - 1)?
        .checked_div(part_size)?;
    let count = i32::try_from(count).ok()?;
    (count <= MAX_MULTIPART_PARTS).then_some(count)
}

pub fn multipart_part_len(byte_len: i64, part_size: i64, part_number: i32) -> Option<i64> {
    let count = multipart_part_count(byte_len, part_size)?;
    if !(1..=count).contains(&part_number) {
        return None;
    }
    if part_number < count {
        Some(part_size)
    } else {
        byte_len.checked_sub(i64::from(count - 1).checked_mul(part_size)?)
    }
}

fn client(config: &S3Config, endpoint: &str) -> aws_sdk_s3::Client {
    let credentials = Credentials::new(
        config.access_key.clone(),
        config.secret_key.expose_secret().to_owned(),
        None,
        None,
        "notegate-s3",
    );
    let mut builder = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(config.region.clone()))
        .endpoint_url(endpoint)
        .credentials_provider(credentials)
        .force_path_style(config.force_path_style);
    if endpoint.starts_with("http://") {
        let mut connector = hyper_legacy::client::HttpConnector::new();
        connector.enforce_http(true);
        builder = builder.http_client(HyperClientBuilder::new().build(connector));
    }
    aws_sdk_s3::Client::from_conf(builder.build())
}

fn presigning_config(ttl: Duration) -> Result<PresigningConfig, ObjectStorageError> {
    PresigningConfig::expires_in(ttl).map_err(|error| {
        tracing::error!(event = "object_storage.presign_config_failed", %error);
        ObjectStorageError::Unavailable
    })
}

fn unavailable(operation: &'static str, error: impl std::fmt::Display) -> ObjectStorageError {
    tracing::error!(event = "object_storage.request_failed", operation, %error);
    ObjectStorageError::Unavailable
}

fn rfc5987_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            out.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{MULTIPART_PART_SIZE, multipart_part_count, multipart_part_len, rfc5987_encode};

    #[test]
    fn encodes_download_filename() {
        assert_eq!(
            rfc5987_encode("한글 파일.txt"),
            "%ED%95%9C%EA%B8%80%20%ED%8C%8C%EC%9D%BC.txt"
        );
    }

    #[test]
    fn derives_multipart_geometry_without_stored_part_rows() {
        let byte_len = MULTIPART_PART_SIZE * 2 + 7;
        assert_eq!(multipart_part_count(byte_len, MULTIPART_PART_SIZE), Some(3));
        assert_eq!(
            multipart_part_len(byte_len, MULTIPART_PART_SIZE, 1),
            Some(MULTIPART_PART_SIZE)
        );
        assert_eq!(
            multipart_part_len(byte_len, MULTIPART_PART_SIZE, 3),
            Some(7)
        );
        assert_eq!(multipart_part_len(byte_len, MULTIPART_PART_SIZE, 4), None);
    }
}
