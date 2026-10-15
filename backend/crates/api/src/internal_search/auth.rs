use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub(super) const TIMESTAMP_HEADER: &str = "x-notegate-internal-timestamp";
pub(super) const REQUEST_SIGNATURE_HEADER: &str = "x-notegate-internal-signature";
pub(super) const RESPONSE_SIGNATURE_HEADER: &str = "x-notegate-internal-response-signature";
pub(super) const BOUND_RESPONSE_SIGNATURE_HEADER: &str = "x-notegate-internal-bound-response-signature";
const MAX_CLOCK_SKEW_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy)]
pub(super) struct RequestBinding {
    pub(super) timestamp: i64,
    digest: [u8; 32],
}

impl RequestBinding {
    pub(super) fn new(timestamp: i64, method: &str, path: &str, body: &[u8]) -> Self {
        Self {
            timestamp,
            digest: Sha256::digest(request_canonical(timestamp, method, path, body)).into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum InternalAuthError {
    #[error("invalid internal signing key")]
    InvalidKey,
    #[error("system clock is before the Unix epoch")]
    InvalidClock,
}

#[derive(Clone)]
pub(super) struct InternalSearchAuth {
    key: [u8; 32],
}

impl InternalSearchAuth {
    pub(super) const fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub(super) fn now_timestamp() -> Result<i64, InternalAuthError> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_error| InternalAuthError::InvalidClock)?
            .as_secs();
        i64::try_from(seconds).map_err(|_error| InternalAuthError::InvalidClock)
    }

    pub(super) fn sign_request(
        &self,
        timestamp: i64,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<String, InternalAuthError> {
        self.sign(&request_canonical(timestamp, method, path, body))
    }

    pub(super) fn verify_request_at(
        &self,
        now: i64,
        timestamp: i64,
        method: &str,
        path: &str,
        body: &[u8],
        signature: &str,
    ) -> bool {
        if now.abs_diff(timestamp) > MAX_CLOCK_SKEW_SECONDS {
            return false;
        }
        self.verify(&request_canonical(timestamp, method, path, body), signature)
    }

    pub(super) fn sign_response(
        &self,
        request_timestamp: i64,
        status: u16,
        path: &str,
        body: &[u8],
    ) -> Result<String, InternalAuthError> {
        self.sign(&response_canonical(request_timestamp, status, path, body))
    }

    #[cfg(test)]
    pub(super) fn verify_response(
        &self,
        request_timestamp: i64,
        status: u16,
        path: &str,
        body: &[u8],
        signature: &str,
    ) -> bool {
        self.verify(
            &response_canonical(request_timestamp, status, path, body),
            signature,
        )
    }

    pub(super) fn sign_bound_response(
        &self,
        request: &RequestBinding,
        status: u16,
        path: &str,
        body: &[u8],
    ) -> Result<String, InternalAuthError> {
        self.sign(&bound_response_canonical(request, status, path, body))
    }

    pub(super) fn verify_bound_response(
        &self,
        request: &RequestBinding,
        status: u16,
        path: &str,
        body: &[u8],
        signature: &str,
    ) -> bool {
        self.verify(
            &bound_response_canonical(request, status, path, body),
            signature,
        )
    }

    fn sign(&self, canonical: &[u8]) -> Result<String, InternalAuthError> {
        let mut mac = <HmacSha256 as hmac::KeyInit>::new_from_slice(&self.key)
            .map_err(|_error| InternalAuthError::InvalidKey)?;
        mac.update(canonical);
        Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }

    fn verify(&self, canonical: &[u8], signature: &str) -> bool {
        let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
            return false;
        };
        let Ok(mut mac) = <HmacSha256 as hmac::KeyInit>::new_from_slice(&self.key) else {
            return false;
        };
        mac.update(canonical);
        mac.verify_slice(&signature).is_ok()
    }
}

fn request_canonical(timestamp: i64, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let mut value = format!("notegate-internal-search-request:v1\n{timestamp}\n{method}\n{path}\n")
        .into_bytes();
    value.extend_from_slice(body);
    value
}

fn response_canonical(request_timestamp: i64, status: u16, path: &str, body: &[u8]) -> Vec<u8> {
    let mut value =
        format!("notegate-internal-search-response:v1\n{request_timestamp}\n{status}\n{path}\n")
            .into_bytes();
    value.extend_from_slice(body);
    value
}

fn bound_response_canonical(
    request: &RequestBinding,
    status: u16,
    path: &str,
    body: &[u8],
) -> Vec<u8> {
    let digest = URL_SAFE_NO_PAD.encode(request.digest);
    let mut value = format!(
        "notegate-internal-search-response:v2\n{}\n{status}\n{path}\n{digest}\n",
        request.timestamp,
    )
    .into_bytes();
    value.extend_from_slice(body);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_signature_binds_time_method_path_and_body() -> Result<(), InternalAuthError> {
        let auth = InternalSearchAuth::new([7; 32]);
        let body = br#"{"q":"needle"}"#;
        let signature = auth.sign_request(1_000, "POST", super::super::FIND_PATH, body)?;

        assert!(auth.verify_request_at(
            1_030,
            1_000,
            "POST",
            super::super::FIND_PATH,
            body,
            &signature,
        ));
        assert!(!auth.verify_request_at(
            1_061,
            1_000,
            "POST",
            super::super::FIND_PATH,
            body,
            &signature,
        ));
        assert!(!auth.verify_request_at(
            1_000,
            1_000,
            "POST",
            super::super::GREP_PATH,
            body,
            &signature,
        ));
        assert!(!auth.verify_request_at(
            1_000,
            1_000,
            "POST",
            super::super::FIND_PATH,
            br#"{"q":"tampered"}"#,
            &signature,
        ));
        Ok(())
    }

    #[test]
    fn response_signature_binds_status_path_and_body() -> Result<(), InternalAuthError> {
        let auth = InternalSearchAuth::new([9; 32]);
        let body = br#"{"items":[]}"#;
        let signature = auth.sign_response(1_000, 200, super::super::FIND_PATH, body)?;

        assert!(auth.verify_response(1_000, 200, super::super::FIND_PATH, body, &signature));
        assert!(!auth.verify_response(1_000, 500, super::super::FIND_PATH, body, &signature));
        assert!(!auth.verify_response(
            1_000,
            200,
            super::super::FIND_PATH,
            br#"{"items":[{}]}"#,
            &signature,
        ));
        Ok(())
    }

    #[test]
    fn bound_response_rejects_other_requests_in_the_same_second() -> Result<(), InternalAuthError> {
        let auth = InternalSearchAuth::new([9; 32]);
        let path = super::super::FIND_PATH;
        let request = br#"{"caller":"alice","space":"one","q":"needle"}"#;
        let binding = RequestBinding::new(1_000, "POST", path, request);
        let other_requests: &[&[u8]] = &[
            br#"{"caller":"bob","space":"one","q":"needle"}"#,
            br#"{"caller":"alice","space":"two","q":"needle"}"#,
            br#"{"caller":"alice","space":"one","q":"other"}"#,
        ];
        let body = br#"{"items":[]}"#;

        for status in [200, 403, 504] {
            let signature = auth.sign_bound_response(&binding, status, path, body)?;
            assert!(auth.verify_bound_response(&binding, status, path, body, &signature));
            for request in other_requests {
                let other = RequestBinding::new(1_000, "POST", path, request);
                assert!(!auth.verify_bound_response(&other, status, path, body, &signature));
            }
            for other in [
                RequestBinding::new(1_001, "POST", path, request),
                RequestBinding::new(1_000, "GET", path, request),
                RequestBinding::new(1_000, "POST", super::super::GREP_PATH, request),
            ] {
                assert!(!auth.verify_bound_response(&other, status, path, body, &signature));
            }
            assert!(!auth.verify_bound_response(&binding, 500, path, body, &signature));
            assert!(!auth.verify_bound_response(
                &binding,
                status,
                super::super::GREP_PATH,
                body,
                &signature,
            ));
            assert!(!auth.verify_bound_response(&binding, status, path, b"{}", &signature));

            let legacy = auth.sign_response(1_000, status, path, body)?;
            assert!(!auth.verify_bound_response(&binding, status, path, body, &legacy));
            assert!(!auth.verify_response(1_000, status, path, body, &signature));
        }
        Ok(())
    }
}
