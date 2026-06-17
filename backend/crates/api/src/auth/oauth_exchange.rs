use openidconnect::core::{CoreErrorResponseType, CoreUserInfoClaims};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, Nonce, OAuth2TokenResponse, PkceCodeVerifier, RefreshToken,
    RequestTokenError, StandardErrorResponse, TokenResponse as OidcTokenResponse,
};
use serde::{Deserialize, Serialize};

use crate::auth::oidc::OidcProvider;

#[derive(Debug, Deserialize, Serialize)]
pub struct UserInfo {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginUserInfo {
    pub userinfo: UserInfo,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RefreshedUserInfo {
    pub userinfo: UserInfo,
    pub refresh_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshUserInfoError {
    #[error("refresh token rejected: {0}")]
    InvalidGrant(String),
    #[error("refresh temporarily unavailable: {message}")]
    Transient {
        message: String,
        rotated_refresh_token: Option<String>,
    },
}

impl RefreshUserInfoError {
    fn transient(message: impl Into<String>) -> Self {
        Self::Transient {
            message: message.into(),
            rotated_refresh_token: None,
        }
    }

    fn transient_after_rotation(
        message: impl Into<String>,
        rotated_refresh_token: Option<String>,
    ) -> Self {
        Self::Transient {
            message: message.into(),
            rotated_refresh_token,
        }
    }
}

pub(super) async fn exchange_code_for_userinfo(
    oidc: &OidcProvider,
    http: &reqwest::Client,
    code: &str,
    verifier: &str,
    nonce: &str,
) -> notegate_core::Result<LoginUserInfo> {
    let client = oidc.client().await?;
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_owned()))
        .map_err(|error| {
            notegate_core::Error::internal(format!("openid token endpoint unavailable: {error}"))
        })?
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
        .request_async(http)
        .await
        .map_err(|error| {
            notegate_core::Error::internal(format!("openid token exchange failed: {error}"))
        })?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| notegate_core::Error::internal("openid token response missing id_token"))?;
    let id_token_verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&id_token_verifier, &Nonce::new(nonce.to_owned()))
        .map_err(|error| {
            notegate_core::Error::internal(format!("id_token verification failed: {error}"))
        })?;

    if let Some(expected_access_token_hash) = claims.access_token_hash() {
        let actual_access_token_hash = AccessTokenHash::from_token(
            token_response.access_token(),
            id_token.signing_alg().map_err(|error| {
                notegate_core::Error::internal(format!("id_token signing alg failed: {error}"))
            })?,
            id_token.signing_key(&id_token_verifier).map_err(|error| {
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

    let refresh_token = token_response
        .refresh_token()
        .ok_or_else(|| {
            notegate_core::Error::internal("openid token response missing refresh_token")
        })?
        .secret()
        .to_owned();
    let userinfo: CoreUserInfoClaims = client
        .user_info(
            token_response.access_token().to_owned(),
            Some(claims.subject().to_owned()),
        )
        .map_err(|error| {
            notegate_core::Error::internal(format!("userinfo endpoint unavailable: {error}"))
        })?
        .request_async(http)
        .await
        .map_err(|error| {
            notegate_core::Error::internal(format!("userinfo request failed: {error}"))
        })?;

    Ok(LoginUserInfo {
        userinfo: into_userinfo(userinfo),
        refresh_token,
    })
}

pub(super) async fn refresh_userinfo(
    oidc: &OidcProvider,
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<RefreshedUserInfo, RefreshUserInfoError> {
    let client = oidc
        .client()
        .await
        .map_err(|error| RefreshUserInfoError::transient(error.to_string()))?;
    let token_response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_owned()))
        .map_err(|error| {
            RefreshUserInfoError::transient(format!("openid refresh endpoint unavailable: {error}"))
        })?
        .request_async(http)
        .await
        .map_err(map_refresh_exchange_error)?;
    let rotated_refresh_token = token_response
        .refresh_token()
        .map(|token| token.secret().to_owned());
    let userinfo: CoreUserInfoClaims = client
        .user_info(token_response.access_token().to_owned(), None)
        .map_err(|error| {
            RefreshUserInfoError::transient_after_rotation(
                format!("userinfo endpoint unavailable: {error}"),
                rotated_refresh_token.clone(),
            )
        })?
        .request_async(http)
        .await
        .map_err(|error| {
            RefreshUserInfoError::transient_after_rotation(
                format!("userinfo request failed: {error}"),
                rotated_refresh_token.clone(),
            )
        })?;
    Ok(RefreshedUserInfo {
        userinfo: into_userinfo(userinfo),
        refresh_token: rotated_refresh_token,
    })
}

fn map_refresh_exchange_error<RE>(
    error: RequestTokenError<RE, StandardErrorResponse<CoreErrorResponseType>>,
) -> RefreshUserInfoError
where
    RE: std::error::Error + 'static,
{
    match error {
        RequestTokenError::ServerResponse(response)
            if response.error().as_ref() == "invalid_grant" =>
        {
            RefreshUserInfoError::InvalidGrant(response.to_string())
        }
        other => {
            RefreshUserInfoError::transient(format!("openid refresh exchange failed: {other}"))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("request failed")]
    struct TestRequestError;

    #[test]
    fn refresh_error_maps_only_invalid_grant_to_permanent_failure() {
        let invalid_grant = RequestTokenError::<
            TestRequestError,
            StandardErrorResponse<CoreErrorResponseType>,
        >::ServerResponse(StandardErrorResponse::new(
            CoreErrorResponseType::InvalidGrant,
            None,
            None,
        ));
        assert!(matches!(
            map_refresh_exchange_error(invalid_grant),
            RefreshUserInfoError::InvalidGrant(_)
        ));

        let invalid_client = RequestTokenError::<
            TestRequestError,
            StandardErrorResponse<CoreErrorResponseType>,
        >::ServerResponse(StandardErrorResponse::new(
            CoreErrorResponseType::InvalidClient,
            None,
            None,
        ));
        assert!(matches!(
            map_refresh_exchange_error(invalid_client),
            RefreshUserInfoError::Transient {
                rotated_refresh_token: None,
                ..
            }
        ));

        let request_failure = RequestTokenError::<
            TestRequestError,
            StandardErrorResponse<CoreErrorResponseType>,
        >::Other("timeout".to_owned());
        assert!(matches!(
            map_refresh_exchange_error(request_failure),
            RefreshUserInfoError::Transient {
                rotated_refresh_token: None,
                ..
            }
        ));
    }
}
