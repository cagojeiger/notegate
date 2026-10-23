//! Presentation only: authentication, consent decisions, and revocation stay in the handlers.
use axum::http::StatusCode;
use axum::response::Response;
use notegate_db::oauth_repo::{Authorization, OAuthGrant};

use crate::auth::page::{browser_page, escape_html};

pub(super) fn consent(auth: &Authorization, account: &str, redirect: &url::Url) -> Response {
    let offline = if auth.scope.split_whitespace().any(|s| s == "offline_access") {
        "<li>Reconnect without signing in again until the grant expires or you revoke it.</li>"
    } else {
        ""
    };
    browser_page(
        StatusCode::OK,
        "Connect application",
        &format!(
            r#"<p class="auth-account">Signed in as <strong>{}</strong></p>
<p><strong>{}</strong> requests access to your NoteGate documents.</p>
<p>Returns to <code>{}</code></p>
<ul class="auth-permissions"><li>Read and change documents in Spaces with MCP &amp; API access enabled. Existing permissions and write locks still apply.</li>{offline}</ul>
<section class="auth-note" aria-labelledby="connection-safety-title"><h2 id="connection-safety-title">Before you allow access</h2>
<p>Only approve a connection you started in an application you recognize. For a localhost or loopback redirect, that application must be running on this device.</p></section>
<details><summary>Connection details</summary><dl>
<dt>Client</dt><dd><code>{}</code></dd><dt>Redirect</dt><dd><code>{}</code></dd>
<dt>Resource</dt><dd><code>{}</code></dd><dt>Scopes</dt><dd><code>{}</code></dd></dl></details>
<form method="post" action="/oauth/approve"><input type="hidden" name="id" value="{}">
<div class="auth-actions"><button type="submit" name="decision" value="approve">Allow access</button><button type="submit" name="decision" value="deny">Deny</button></div></form>"#,
            escape_html(account),
            escape_html(&auth.client_name),
            escape_html(&redirect.origin().ascii_serialization()),
            escape_html(&auth.client_id),
            escape_html(&auth.redirect_uri),
            escape_html(&auth.resource),
            escape_html(&auth.scope),
            auth.id,
        ),
        Some(redirect),
    )
}

pub(super) fn device(account: &str, code: &str) -> Response {
    browser_page(
        StatusCode::OK,
        "Connect NoteGate CLI",
        &format!(
            r#"<p class="auth-account">Signed in as <strong>{}</strong></p>
<p>Connect the NoteGate CLI running in your terminal.</p>
<ul class="auth-permissions"><li>Read and change documents in enabled Spaces, subject to existing permissions.</li><li>Reconnect without signing in again until the grant expires or you revoke it.</li></ul>
<section class="auth-note" aria-labelledby="device-safety-title"><h2 id="device-safety-title">Check the code</h2>
<p>Only approve a code displayed by a CLI login you started.</p></section>
<form method="post" action="/oauth/device"><label class="auth-field">Code shown in your terminal
<input name="user_code" maxlength="32" required autocomplete="off" autocapitalize="characters" spellcheck="false" value="{}"></label>
<div class="auth-actions"><button type="submit" name="decision" value="approve">Connect CLI</button><button type="submit" name="decision" value="deny">Deny</button></div></form>"#,
            escape_html(account),
            escape_html(code),
        ),
        None,
    )
}

pub(super) fn connections(account: &str, grants: &[OAuthGrant]) -> Response {
    let mut body = format!(
        r#"<p class="auth-account">Signed in as <strong>{}</strong></p><p>Manage applications connected to your account.</p>
<section class="auth-note" aria-labelledby="revocation-impact-title"><h2 id="revocation-impact-title">Revoking access</h2>
<p>Revoking a connection immediately disables its access and refresh tokens.</p></section>"#,
        escape_html(account)
    );
    if grants.is_empty() {
        body.push_str("<p class=\"auth-empty\">No connected applications. Start a connection from your MCP client or NoteGate CLI.</p>");
    } else {
        body.push_str("<ul class=\"auth-connections\">");
        for grant in grants {
            let badges = grant
                .scope
                .split_whitespace()
                .map(|scope| {
                    let label = match scope {
                        "mcp" => "MCP",
                        "cli" => "CLI",
                        "offline_access" => "Offline access",
                        scope => scope,
                    };
                    format!("<span class=\"auth-badge\">{}</span>", escape_html(label))
                })
                .collect::<String>();
            let created = grant.created_at.to_rfc3339();
            let expires = grant.expires_at.to_rfc3339();
            body.push_str(&format!(
                r#"<li class="auth-connection"><h2>{}</h2><div class="auth-badges">{badges}</div>
<details><summary>Connection details</summary><dl><dt>Client</dt><dd><code>{}</code></dd>
<dt>Resource</dt><dd><code>{}</code></dd><dt>Scopes</dt><dd><code>{}</code></dd>
<dt>Connected</dt><dd><time data-auth-time datetime="{created}">{created}</time></dd>
<dt>Grant expires</dt><dd><time data-auth-time datetime="{expires}">{expires}</time></dd></dl></details>
<form method="post" action="/oauth/connections/revoke"><input type="hidden" name="id" value="{}">
<button type="submit" class="auth-danger">Revoke connection</button></form></li>"#,
                escape_html(&grant.client_name), escape_html(&grant.client_id),
                escape_html(&grant.resource), escape_html(&grant.scope), grant.id,
            ));
        }
        body.push_str("</ul>");
    }
    browser_page(StatusCode::OK, "Connected applications", &body, None)
}
