-- Bind the legacy subject namespace once during API startup. It must not be
-- reinterpreted when another OIDC provider is configured later.
CREATE TABLE legacy_identity_issuer (
    singleton BOOLEAN PRIMARY KEY DEFAULT true CHECK (singleton),
    issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 2048)
);

CREATE TABLE external_identities (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 2048),
    subject_hash TEXT NOT NULL,
    subject_hash_key_id TEXT NOT NULL REFERENCES crypto_key_epochs(key_id),
    subject_hash_version INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, subject_hash)
);

CREATE INDEX external_identities_user_idx ON external_identities(user_id);
