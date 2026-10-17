ALTER TABLE browser_sessions ADD COLUMN local_session BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE oidc_login_flows (
    id UUID PRIMARY KEY,
    cookie_hash TEXT NOT NULL,
    issuer TEXT NOT NULL,
    private_payload JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX oidc_login_flows_expiry_idx ON oidc_login_flows(expires_at);

CREATE TABLE oauth_authorizations (
    id UUID PRIMARY KEY,
    client_id TEXT NOT NULL,
    client_name TEXT NOT NULL,
    resource TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL,
    code_challenge TEXT NOT NULL CHECK (length(code_challenge) = 43),
    cookie_hash TEXT NOT NULL,
    private_payload JSONB NOT NULL,
    user_id UUID REFERENCES users(id),
    code_hash TEXT UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    CHECK ((user_id IS NULL) = (code_hash IS NULL))
);
CREATE INDEX oauth_authorizations_expiry_idx ON oauth_authorizations(expires_at);

CREATE TABLE oauth_grants (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    client_id TEXT NOT NULL,
    client_name TEXT NOT NULL,
    resource TEXT NOT NULL,
    scope TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    idle_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX oauth_grants_user_idx ON oauth_grants(user_id, created_at DESC);
CREATE INDEX oauth_grants_expiry_idx ON oauth_grants(expires_at);

CREATE TABLE oauth_credentials (
    token_hash TEXT PRIMARY KEY,
    hash_key_id TEXT NOT NULL REFERENCES crypto_key_epochs(key_id),
    hash_version INTEGER NOT NULL,
    grant_id UUID NOT NULL REFERENCES oauth_grants(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ
);
CREATE INDEX oauth_credentials_grant_idx ON oauth_credentials(grant_id);

CREATE TABLE oauth_device_codes (
    device_hash TEXT PRIMARY KEY,
    user_code_hash TEXT UNIQUE NOT NULL,
    client_id TEXT NOT NULL,
    resource TEXT NOT NULL,
    scope TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    next_poll_at TIMESTAMPTZ NOT NULL,
    poll_interval_seconds INTEGER NOT NULL DEFAULT 5 CHECK (poll_interval_seconds >= 5),
    user_id UUID REFERENCES users(id),
    denied_at TIMESTAMPTZ,
    consumed_at TIMESTAMPTZ
);
CREATE INDEX oauth_device_codes_expiry_idx ON oauth_device_codes(expires_at);
