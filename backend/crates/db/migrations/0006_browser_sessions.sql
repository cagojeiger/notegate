CREATE TABLE browser_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    hash_key_id TEXT NOT NULL REFERENCES crypto_key_epochs(key_id),
    hash_version INTEGER NOT NULL DEFAULT 1,
    refresh_token_ciphertext BYTEA NOT NULL,
    refresh_token_nonce BYTEA NOT NULL,
    refresh_token_enc_key_id TEXT NOT NULL REFERENCES crypto_key_epochs(key_id),
    refresh_token_enc_version INTEGER NOT NULL DEFAULT 1,
    validated_until TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ,
    last_refreshed_at TIMESTAMPTZ,
    refresh_started_at TIMESTAMPTZ,
    refresh_claim_id UUID,
    revoked_at TIMESTAMPTZ,
    revoked_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (validated_until <= expires_at),
    CHECK (
        (refresh_started_at IS NULL AND refresh_claim_id IS NULL)
        OR (refresh_started_at IS NOT NULL AND refresh_claim_id IS NOT NULL)
    ),
    CHECK (
        (revoked_at IS NULL AND revoked_reason IS NULL)
        OR revoked_at IS NOT NULL
    )
);

CREATE INDEX browser_sessions_user_created_idx ON browser_sessions(user_id, created_at DESC, id DESC);
CREATE INDEX browser_sessions_live_idx ON browser_sessions(expires_at) WHERE revoked_at IS NULL;
