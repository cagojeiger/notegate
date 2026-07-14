//! S3-compatible object storage access and presigned transfer URLs.

use std::collections::BTreeMap;
use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_smithy_http_client::hyper_014::HyperClientBuilder;
use notegate_core::S3Config;
use secrecy::ExposeSecret as _;

use crate::error::ApiError;

const TRANSFER_URL_TTL: Duration = Duration::from_secs(15 * 60);

pub struct PresignedPut {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStorageError {
    Missing,
    SizeMismatch,
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

    pub async fn presign_put(
        &self,
        object_key: &str,
        content_type: &str,
    ) -> Result<PresignedPut, ObjectStorageError> {
        let presigned = self
            .public
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .content_type(content_type)
            .if_none_match("*")
            .presigned(presigning_config()?)
            .await
            .map_err(|error| unavailable("presign_put", error))?;
        Ok(PresignedPut {
            url: presigned.uri().to_owned(),
            headers: presigned
                .headers()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        })
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
        let mut request = self
            .public
            .get_object()
            .bucket(&self.bucket)
            .key(object_key);
        if let Some(filename) = filename {
            request = request.response_content_disposition(format!(
                "attachment; filename*=UTF-8''{}",
                rfc5987_encode(filename)
            ));
        }
        let presigned = request
            .presigned(presigning_config()?)
            .await
            .map_err(|error| unavailable("presign_get", error))?;
        Ok(presigned.uri().to_owned())
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

fn presigning_config() -> Result<PresigningConfig, ObjectStorageError> {
    PresigningConfig::expires_in(TRANSFER_URL_TTL).map_err(|error| {
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
    use super::rfc5987_encode;

    #[test]
    fn encodes_download_filename() {
        assert_eq!(
            rfc5987_encode("한글 파일.txt"),
            "%ED%95%9C%EA%B8%80%20%ED%8C%8C%EC%9D%BC.txt"
        );
    }
}
