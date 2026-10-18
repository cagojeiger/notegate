//! Cached OIDC client construction.
//!
//! Discovery metadata, including browser ID-token signing keys, is cached for
//! an hour. A callback can refresh just the trusted provider's JWKS when its
//! token cannot be verified with the cached keys.

use std::time::{Duration, Instant};

use openidconnect::core::{CoreClient, CoreJsonWebKeySet, CoreProviderMetadata};
use openidconnect::{AuthType, ClientId, HttpRequest, HttpResponse, IssuerUrl, RedirectUrl};
use tokio::sync::{Mutex, RwLock};

/// Freshness interval for discovered metadata. Failed discovery may continue
/// serving the previous snapshot; successful key-only refreshes do not extend it.
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
    client_secret: Option<secrecy::SecretString>,
    redirect_url: String,
    http: reqwest::Client,
    cache: RwLock<MetadataCache>,
    refresh_lock: Mutex<()>,
}

#[derive(Clone, Default)]
struct MetadataCache {
    metadata: Option<CachedMetadata>,
    // Failed attempts count too: callbacks sharing a snapshot share its retry.
    attempt_epoch: u64,
}

#[derive(Clone)]
struct CachedMetadata {
    metadata: CoreProviderMetadata,
    fetched_at: Instant,
}

pub(crate) struct OidcClientSnapshot {
    pub(crate) client: OidcClient,
    pub(crate) attempt_epoch: u64,
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
            issuer: config.oidc.issuer.clone(),
            client_id: config.oidc.client_id.clone(),
            client_secret: config.oidc.client_secret.clone(),
            redirect_url: config.oauth_redirect_url.clone(),
            http,
            cache: RwLock::new(MetadataCache::default()),
            refresh_lock: Mutex::new(()),
        }
    }

    /// The configured issuer whose discovery and ID tokens are verified.
    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Build an OIDC client from cached (or freshly discovered) provider metadata.
    pub(crate) async fn client(&self) -> notegate_core::Result<OidcClient> {
        Ok(self.client_snapshot().await?.client)
    }

    pub(crate) async fn client_snapshot(&self) -> notegate_core::Result<OidcClientSnapshot> {
        let cached = self.metadata().await?;
        self.snapshot_client(cached)
    }

    fn snapshot_client(&self, cached: MetadataCache) -> notegate_core::Result<OidcClientSnapshot> {
        let metadata = cached.metadata.ok_or_else(|| {
            notegate_core::Error::internal("openid discovery has no cached metadata")
        })?;
        Ok(OidcClientSnapshot {
            client: self.client_from_metadata(metadata.metadata)?,
            attempt_epoch: cached.attempt_epoch,
        })
    }

    fn client_from_metadata(
        &self,
        metadata: CoreProviderMetadata,
    ) -> notegate_core::Result<OidcClient> {
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            self.client_secret.as_ref().map(|secret| {
                use secrecy::ExposeSecret as _;
                openidconnect::ClientSecret::new(secret.expose_secret().to_owned())
            }),
        )
        .set_redirect_uri(
            RedirectUrl::new(self.redirect_url.clone()).map_err(|error| {
                notegate_core::Error::validation(format!("invalid redirect URL: {error}"))
            })?,
        )
        .set_auth_type(AuthType::RequestBody);
        Ok(client)
    }

    async fn metadata(&self) -> notegate_core::Result<MetadataCache> {
        let snapshot = { self.cache.read().await.clone() };
        if let Some(cached) = &snapshot.metadata
            && cached.fetched_at.elapsed() <= METADATA_CACHE_TTL
        {
            return Ok(snapshot);
        }

        let _refresh = self.refresh_lock.lock().await;
        let current = { self.cache.read().await.clone() };
        // Another discovery or JWKS attempt already handled this snapshot.
        if current.attempt_epoch != snapshot.attempt_epoch {
            return if current.metadata.is_some() {
                Ok(current)
            } else {
                Err(notegate_core::Error::internal(
                    "openid discovery failed during concurrent refresh",
                ))
            };
        }
        if let Some(cached) = &current.metadata
            && cached.fetched_at.elapsed() <= METADATA_CACHE_TTL
        {
            return Ok(current);
        }

        let result = self.discover().await;
        let mut cache = self.cache.write().await;
        cache.attempt_epoch = cache.attempt_epoch.wrapping_add(1);
        match result {
            Ok(metadata) => {
                cache.metadata = Some(CachedMetadata {
                    metadata,
                    fetched_at: Instant::now(),
                });
                Ok(cache.clone())
            }
            Err(error) => match &cache.metadata {
                // Serve stale metadata if discovery is momentarily unavailable;
                // endpoints rarely change, so this keeps logins working.
                Some(_) => {
                    tracing::warn!(event = "oidc.discovery_stale", %error);
                    Ok(cache.clone())
                }
                None => Err(error),
            },
        }
    }

    /// Refresh only keys from the previously discovered JWKS URI. A token's
    /// headers never choose the URL, and a key refresh does not extend discovery TTL.
    pub(crate) async fn refresh_jwks_after(
        &self,
        attempt_epoch: u64,
    ) -> notegate_core::Result<OidcClient> {
        let _refresh = self.refresh_lock.lock().await;
        let current = { self.cache.read().await.clone() };
        if current.attempt_epoch != attempt_epoch {
            return Ok(self.snapshot_client(current)?.client);
        }
        let cached = current.metadata.ok_or_else(|| {
            notegate_core::Error::internal("openid JWKS refresh has no cached metadata")
        })?;
        let http_client = self.http.clone();
        let http = move |request| {
            let http_client = http_client.clone();
            async move { execute_oidc_request(&http_client, request).await }
        };
        let result = CoreJsonWebKeySet::fetch_async(cached.metadata.jwks_uri(), &http).await;
        let mut cache = self.cache.write().await;
        cache.attempt_epoch = cache.attempt_epoch.wrapping_add(1);
        match result {
            Ok(jwks) => {
                let metadata = cached.metadata.set_jwks(jwks);
                cache.metadata = Some(CachedMetadata {
                    metadata: metadata.clone(),
                    fetched_at: cached.fetched_at,
                });
                self.client_from_metadata(metadata)
            }
            Err(error) => Err(notegate_core::Error::internal(format!(
                "openid JWKS refresh failed: {error}"
            ))),
        }
    }

    async fn discover(&self) -> notegate_core::Result<CoreProviderMetadata> {
        let issuer = IssuerUrl::new(self.issuer.clone()).map_err(|error| {
            notegate_core::Error::validation(format!("invalid issuer URL: {error}"))
        })?;
        let http_client = self.http.clone();
        let http = move |request| {
            let http_client = http_client.clone();
            async move { execute_oidc_request(&http_client, request).await }
        };
        CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|error| {
                notegate_core::Error::internal(format!("openid discovery failed: {error}"))
            })
    }
}

#[cfg(test)]
#[path = "oidc_tests.rs"]
mod cache_tests;

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
