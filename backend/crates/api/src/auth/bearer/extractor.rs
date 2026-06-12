use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum_extra::extract::CookieJar;

pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() { None } else { Some(token) }
}

pub fn extract_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let jar = CookieJar::from_headers(headers);
    let value = jar.get(name)?.value().trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use axum::http::HeaderMap;
    use axum::http::header::{AUTHORIZATION, COOKIE};

    use super::{extract_bearer, extract_cookie_value};

    #[test]
    fn extracts_bearer_token() -> Result<(), Box<dyn std::error::Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer abc".parse()?);
        assert_eq!(extract_bearer(&headers), Some("abc"));
        Ok(())
    }

    #[test]
    fn rejects_missing_or_empty_bearer() -> Result<(), Box<dyn std::error::Error>> {
        let mut headers = HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);
        headers.insert(AUTHORIZATION, "Bearer   ".parse()?);
        assert_eq!(extract_bearer(&headers), None);
        headers.insert(AUTHORIZATION, "Basic abc".parse()?);
        assert_eq!(extract_bearer(&headers), None);
        Ok(())
    }

    #[test]
    fn extracts_cookie_value() -> Result<(), Box<dyn std::error::Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, "preview_cookie=abc".parse()?);
        assert_eq!(
            extract_cookie_value(&headers, "preview_cookie").as_deref(),
            Some("abc")
        );
        Ok(())
    }
}
