use axum_extra::extract::cookie::{Cookie, SameSite};
use openidconnect::core::CoreAuthenticationFlow;
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge, Scope};
use time::Duration as CookieDuration;

use crate::auth::oidc::OidcProvider;

pub(super) struct LoginFlow {
    pub redirect_url: String,
    pub csrf_state: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

pub(super) async fn new_login_flow(oidc: &OidcProvider) -> notegate_core::Result<LoginFlow> {
    let client = oidc.client().await?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let builder = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || CsrfToken::new(uuid::Uuid::new_v4().to_string()),
            Nonce::new_random,
        )
        .add_scope(Scope::new("profile".to_owned()))
        .add_scope(Scope::new("email".to_owned()))
        .set_pkce_challenge(pkce_challenge);
    let (redirect_url, csrf_state, nonce) = builder.url();

    Ok(LoginFlow {
        redirect_url: redirect_url.to_string(),
        csrf_state: csrf_state.secret().to_owned(),
        pkce_verifier: pkce_verifier.secret().to_owned(),
        nonce: nonce.secret().to_owned(),
    })
}

/// Build a hardened cookie (path=/, HttpOnly, SameSite=Lax) with the given
/// max-age. Shared by every auth cookie so the security flags never drift.
pub(super) fn hardened_cookie(
    name: impl Into<std::borrow::Cow<'static, str>>,
    value: String,
    max_age_secs: i64,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(CookieDuration::seconds(max_age_secs))
        .build()
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth_cookies_are_hardened_and_expiration_is_explicit() {
        for secure in [true, false] {
            for age in [0, 300] {
                let cookie = super::hardened_cookie("test", "value".to_owned(), age, secure);
                assert_eq!(cookie.http_only(), Some(true));
                assert_eq!(cookie.secure(), Some(secure));
                assert_eq!(
                    cookie.same_site(),
                    Some(axum_extra::extract::cookie::SameSite::Lax)
                );
                assert_eq!(cookie.path(), Some("/"));
                assert_eq!(cookie.max_age(), Some(time::Duration::seconds(age)));
            }
        }
    }
}
