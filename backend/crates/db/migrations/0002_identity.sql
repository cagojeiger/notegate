-- notegate schema: crypto epochs, accounts, users, agents, and API keys.

CREATE TABLE crypto_key_epochs (
    key_id TEXT PRIMARY KEY,
    domain TEXT NOT NULL CHECK (domain IN ('enc', 'lookup')),
    status TEXT NOT NULL CHECK (status IN ('active', 'verify_only', 'revoked')),
    verify_tag TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    activated_at TIMESTAMPTZ,
    retired_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    CHECK (key_id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,126}$'),
    CHECK (
        (status = 'active' AND activated_at IS NOT NULL AND retired_at IS NULL AND revoked_at IS NULL)
        OR (status = 'verify_only' AND activated_at IS NOT NULL AND retired_at IS NOT NULL AND revoked_at IS NULL)
        OR (status = 'revoked' AND revoked_at IS NOT NULL)
    )
);
CREATE UNIQUE INDEX crypto_key_epochs_one_active_per_domain
    ON crypto_key_epochs(domain)
    WHERE status = 'active';

CREATE TABLE accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name_ciphertext BYTEA,
    display_name_nonce BYTEA,
    display_name_enc_key_id TEXT REFERENCES crypto_key_epochs(key_id),
    display_name_enc_version INTEGER,
    is_active BOOLEAN NOT NULL DEFAULT true,
    deleted_at TIMESTAMPTZ,
    deleted_by_account_id UUID REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (display_name_ciphertext IS NULL AND display_name_nonce IS NULL AND display_name_enc_key_id IS NULL AND display_name_enc_version IS NULL)
        OR (display_name_ciphertext IS NOT NULL AND display_name_nonce IS NOT NULL AND display_name_enc_key_id IS NOT NULL AND display_name_enc_version IS NOT NULL)
    ),
    CHECK (
        (deleted_at IS NULL AND deleted_by_account_id IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by_account_id IS NOT NULL)
    )
);

CREATE TABLE users (
    id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    provider_sub_hash TEXT UNIQUE,
    provider_sub_hash_key_id TEXT REFERENCES crypto_key_epochs(key_id),
    provider_sub_hash_version INTEGER,
    email_ciphertext BYTEA,
    email_nonce BYTEA,
    email_enc_key_id TEXT REFERENCES crypto_key_epochs(key_id),
    email_enc_version INTEGER,
    email_hash TEXT,
    email_hash_key_id TEXT REFERENCES crypto_key_epochs(key_id),
    email_hash_version INTEGER,
    tier TEXT NOT NULL DEFAULT 'system_max' CHECK (tier IN ('tier0', 'system_max')),
    anonymized_at TIMESTAMPTZ,
    CHECK (
        (provider_sub_hash IS NULL AND provider_sub_hash_key_id IS NULL AND provider_sub_hash_version IS NULL)
        OR (provider_sub_hash IS NOT NULL AND provider_sub_hash_key_id IS NOT NULL AND provider_sub_hash_version IS NOT NULL)
    ),
    CHECK (
        (email_ciphertext IS NULL AND email_nonce IS NULL AND email_enc_key_id IS NULL AND email_enc_version IS NULL)
        OR (email_ciphertext IS NOT NULL AND email_nonce IS NOT NULL AND email_enc_key_id IS NOT NULL AND email_enc_version IS NOT NULL)
    ),
    CHECK (
        (email_hash IS NULL AND email_hash_key_id IS NULL AND email_hash_version IS NULL)
        OR (email_hash IS NOT NULL AND email_hash_key_id IS NOT NULL AND email_hash_version IS NOT NULL)
    )
);

CREATE TABLE agents (
    id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    owner_user_id UUID NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (char_length(name) BETWEEN 1 AND 63 AND char_length(btrim(name)) >= 1)
);

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_by_user_id UUID NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    token_prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    hash_key_id TEXT NOT NULL REFERENCES crypto_key_epochs(key_id),
    hash_version INTEGER NOT NULL DEFAULT 1,
    scopes TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[] CHECK (cardinality(scopes) = 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    revoked_by_user_id UUID REFERENCES users(id),
    revoked_reason TEXT,
    rotated_from_key_id UUID REFERENCES api_keys(id),
    CHECK (
        (revoked_at IS NULL AND revoked_by_user_id IS NULL AND revoked_reason IS NULL)
        OR (revoked_at IS NOT NULL AND (revoked_by_user_id IS NOT NULL OR revoked_reason IS NOT NULL))
    ),
    CHECK (char_length(name) BETWEEN 1 AND 63 AND char_length(btrim(name)) >= 1)
);
CREATE INDEX api_keys_account_created_idx ON api_keys(account_id, created_at DESC, id DESC);
