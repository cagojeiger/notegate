//! Cached OIDC client construction.
//!
//! Provider discovery (`.well-known/openid-configuration`) returns endpoint
//! addresses that change far less often than signing keys (those are handled
//! separately by [`crate::auth::jwt::JwtAuthority`]). So instead of discovering
//! on every login/callback, we cache the discovered metadata with a generous
//! TTL and rebuild the (cheap, network-free) client per request from it.

use std::time::{Duration, Instant};

use openidconnect::core::{CoreClient, CoreProviderMetadata};
use openidconnect::{AuthType, ClientId, HttpRequest, HttpResponse, IssuerUrl, RedirectUrl};
use tokio::sync::RwLock;

/// How long discovered provider metadata stays fresh. An hour bounds staleness
/// (so an authgate endpoint change is absorbed without a restart) while keeping
/// the per-login discovery cost effectively zero.
const METADATA_CACHE_TTL: Duration = Duration::from_secs(3600);

pub(crate) type OidcClient = CoreClient<
    openidconnect::EndpointSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointNotSet,
    openidconnect::EndpointMaybeSet,
    openidconnect::EndpointMaybeSet,
>;

pub(crate) struct OidcProvider {
    issuer: String,
    client_id: String,
    redirect_url: String,
    http: reqwest::Client,
    cache: RwLock<Option<CachedMetadata>>,
}

#[derive(Clone)]
struct CachedMetadata {
    metadata: CoreProviderMetadata,
    fetched_at: Instant,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OidcHttpError {
    #[error("OIDC HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("OIDC HTTP response could not be built: {0}")]
    Response(#[from] openidconnect::http::Error),
}

pub(crate) async fn execute_oidc_request(
    http: &reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, OidcHttpError> {
    let response = http.execute(reqwest::Request::try_from(request)?).await?;
    let status = response.status();
    let version = response.version();
    let headers = response.headers().clone();
    let body = response.bytes().await?.to_vec();
    let mut response = openidconnect::http::Response::builder()
        .status(status)
        .version(version)
        .body(body)?;
    *response.headers_mut() = headers;
    Ok(response)
}

impl OidcProvider {
    pub(crate) fn new(config: &notegate_core::Config, http: reqwest::Client) -> Self {
        Self {
            issuer: config.authgate_url.clone(),
            client_id: config.oauth_client_id.clone(),
            redirect_url: config.oauth_redirect_url.clone(),
            http,
            cache: RwLock::new(None),
        }
    }

    /// Build an OIDC client from cached (or freshly discovered) provider metadata.
    pub(crate) async fn client(&self) -> notegate_core::Result<OidcClient> {
        let metadata = self.metadata().await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            None,
        )
        .set_redirect_uri(
            RedirectUrl::new(self.redirect_url.clone()).map_err(|error| {
                notegate_core::Error::validation(format!("invalid redirect URL: {error}"))
            })?,
        )
        .set_auth_type(AuthType::RequestBody);
        Ok(client)
    }

    async fn metadata(&self) -> notegate_core::Result<CoreProviderMetadata> {
        let snapshot = { self.cache.read().await.clone() };
        if let Some(cached) = &snapshot
            && cached.fetched_at.elapsed() <= METADATA_CACHE_TTL
        {
            return Ok(cached.metadata.clone());
        }

        match self.refresh().await {
            Ok(metadata) => Ok(metadata),
            Err(error) => match snapshot {
                // Serve stale metadata if discovery is momentarily unavailable;
                // endpoints rarely change, so this keeps logins working.
                Some(cached) => {
                    tracing::warn!(event = "oidc.discovery_stale", %error);
                    Ok(cached.metadata)
                }
                None => Err(error),
            },
        }
    }

    async fn refresh(&self) -> notegate_core::Result<CoreProviderMetadata> {
        let issuer = IssuerUrl::new(self.issuer.clone()).map_err(|error| {
            notegate_core::Error::validation(format!("invalid issuer URL: {error}"))
        })?;
        let http_client = self.http.clone();
        let http = move |request| {
            let http_client = http_client.clone();
            async move { execute_oidc_request(&http_client, request).await }
        };
        let metadata = CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|error| {
                notegate_core::Error::internal(format!("openid discovery failed: {error}"))
            })?;
        {
            let mut guard = self.cache.write().await;
            *guard = Some(CachedMetadata {
                metadata: metadata.clone(),
                fetched_at: Instant::now(),
            });
        }
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Bytes;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;

    use super::*;

    async fn echo_oidc_request(headers: HeaderMap, body: Bytes) -> Response {
        let mut response = (StatusCode::CREATED, body).into_response();
        if let Some(value) = headers.get("x-oidc-request") {
            response
                .headers_mut()
                .insert("x-oidc-response", value.clone());
        }
        response
    }

    #[tokio::test]
    async fn reqwest_adapter_preserves_the_oidc_http_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/oidc", post(echo_oidc_request)),
            )
            .await
        });

        let request = openidconnect::http::Request::builder()
            .method("POST")
            .uri(format!("http://{address}/oidc"))
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-oidc-request", "preserved")
            .body(b"code=example".to_vec())?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let response = execute_oidc_request(&client, request).await?;

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("x-oidc-response"),
            Some(&HeaderValue::from_static("preserved"))
        );
        assert_eq!(response.body(), b"code=example");

        server.abort();
        Ok(())
    }
}
