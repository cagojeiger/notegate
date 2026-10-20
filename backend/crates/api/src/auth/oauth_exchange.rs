use openidconnect::core::CoreUserInfoClaims;
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClaimsVerificationError, Nonce, OAuth2TokenResponse,
    PkceCodeVerifier, SignatureVerificationError, TokenResponse as OidcTokenResponse,
};
use serde::{Deserialize, Serialize};

use crate::auth::oidc::{OidcProvider, execute_oidc_request};

#[derive(Debug, Deserialize, Serialize)]
pub struct UserInfo {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

pub(super) async fn exchange_code_for_userinfo(
    oidc: &OidcProvider,
    http: &reqwest::Client,
    code: &str,
    verifier: &str,
    nonce: &str,
) -> notegate_core::Result<UserInfo> {
    let snapshot = oidc.client_snapshot().await?;
    let client = snapshot.client;
    let http_client = http.clone();
    let http = move |request| {
        let http_client = http_client.clone();
        async move { execute_oidc_request(&http_client, request).await }
    };
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_owned()))
        .map_err(|error| {
            notegate_core::Error::internal(format!("openid token endpoint unavailable: {error}"))
        })?
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
        .request_async(&http)
        .await
        .map_err(|error| {
            notegate_core::Error::internal(format!("openid token exchange failed: {error}"))
        })?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| notegate_core::Error::internal("openid token response missing id_token"))?;
    let id_token_verifier = client.id_token_verifier();
    let expected_nonce = Nonce::new(nonce.to_owned());
    let refreshed_client;
    let refreshed_verifier;
    let (client, id_token_verifier, claims) =
        match id_token.claims(&id_token_verifier, &expected_nonce) {
            Ok(claims) => (&client, &id_token_verifier, claims),
            Err(error) if can_refresh_jwks(&error) => {
                refreshed_client = oidc.refresh_jwks_after(snapshot.attempt_epoch).await?;
                refreshed_verifier = refreshed_client.id_token_verifier();
                let claims = id_token
                    .claims(&refreshed_verifier, &expected_nonce)
                    .map_err(|error| {
                        notegate_core::Error::internal(format!(
                            "id_token verification failed after JWKS refresh: {error}"
                        ))
                    })?;
                (&refreshed_client, &refreshed_verifier, claims)
            }
            Err(error) => {
                return Err(notegate_core::Error::internal(format!(
                    "id_token verification failed: {error}"
                )));
            }
        };

    if let Some(expected_access_token_hash) = claims.access_token_hash() {
        let actual_access_token_hash = AccessTokenHash::from_token(
            token_response.access_token(),
            id_token.signing_alg().map_err(|error| {
                notegate_core::Error::internal(format!("id_token signing alg failed: {error}"))
            })?,
            id_token.signing_key(id_token_verifier).map_err(|error| {
                notegate_core::Error::internal(format!("id_token signing key failed: {error}"))
            })?,
        )
        .map_err(|error| {
            notegate_core::Error::internal(format!("access token hash failed: {error}"))
        })?;
        if actual_access_token_hash != *expected_access_token_hash {
            return Err(notegate_core::Error::internal("access token hash mismatch"));
        }
    }

    let userinfo: CoreUserInfoClaims = client
        .user_info(
            token_response.access_token().to_owned(),
            Some(claims.subject().to_owned()),
        )
        .map_err(|error| {
            notegate_core::Error::internal(format!("userinfo endpoint unavailable: {error}"))
        })?
        .request_async(&http)
        .await
        .map_err(|error| {
            notegate_core::Error::internal(format!("userinfo request failed: {error}"))
        })?;

    Ok(into_userinfo(userinfo))
}

fn can_refresh_jwks(error: &ClaimsVerificationError) -> bool {
    matches!(
        error,
        ClaimsVerificationError::SignatureVerification(
            SignatureVerificationError::NoMatchingKey | SignatureVerificationError::CryptoError(_)
        )
    )
}

fn into_userinfo(userinfo: CoreUserInfoClaims) -> UserInfo {
    UserInfo {
        sub: userinfo.subject().as_str().to_owned(),
        email: userinfo.email().map(|email| email.as_str().to_owned()),
        name: userinfo
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.as_str().to_owned()),
    }
}
