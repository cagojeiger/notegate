-- Initial schema (AI-native personal file spaces).
--
-- accounts is the common authenticated actor; users own spaces/agents, and
-- agents are user-managed workers connected to spaces. Tree location is
-- parent_id + name; full paths are derived, never stored.

CREATE EXTENSION IF NOT EXISTS pgcrypto;   -- gen_random_uuid()

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
CREATE INDEX users_email_hash_idx ON users(email_hash) WHERE email_hash IS NOT NULL;

CREATE TABLE agents (
    id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    owner_user_id UUID NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (char_length(name) BETWEEN 1 AND 63 AND char_length(btrim(name)) >= 1)
);
CREATE INDEX agents_owner_user_live_idx ON agents(owner_user_id, id);

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
CREATE INDEX api_keys_account_live_idx ON api_keys(account_id) WHERE revoked_at IS NULL;
CREATE INDEX api_keys_account_created_idx ON api_keys(account_id, created_at DESC, id DESC);
CREATE INDEX api_keys_hash_key_live_idx ON api_keys(hash_key_id) WHERE revoked_at IS NULL;
CREATE INDEX api_keys_expiring_live_idx ON api_keys(expires_at) WHERE revoked_at IS NULL;
CREATE INDEX api_keys_rotated_from_idx ON api_keys(rotated_from_key_id) WHERE rotated_from_key_id IS NOT NULL;

CREATE TABLE spaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id UUID NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    deleted_by_user_id UUID REFERENCES users(id),
    purge_after TIMESTAMPTZ,
    CHECK (name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$'),
    CHECK (
        (deleted_at IS NULL AND deleted_by_user_id IS NULL AND purge_after IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by_user_id IS NOT NULL AND purge_after IS NOT NULL)
    )
);
CREATE UNIQUE INDEX spaces_owner_name_live_uidx
    ON spaces(owner_user_id, name)
    WHERE deleted_at IS NULL;
CREATE INDEX spaces_owner_list_idx
    ON spaces(owner_user_id, sort_order, name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE space_agent_connections (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN ('read', 'write')),
    connected_by_user_id UUID NOT NULL REFERENCES users(id),
    connected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    disconnected_at TIMESTAMPTZ,
    disconnected_by_user_id UUID REFERENCES users(id),
    PRIMARY KEY (space_id, agent_id),
    CHECK (
        (disconnected_at IS NULL AND disconnected_by_user_id IS NULL)
        OR (disconnected_at IS NOT NULL AND disconnected_by_user_id IS NOT NULL)
    )
);
CREATE INDEX space_agent_connections_agent_live_idx
    ON space_agent_connections(agent_id, space_id)
    WHERE disconnected_at IS NULL;
CREATE INDEX space_agent_connections_space_live_idx
    ON space_agent_connections(space_id, agent_id)
    WHERE disconnected_at IS NULL;

CREATE TABLE nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    parent_id UUID,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('folder', 'text', 'file')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by_account_id UUID NOT NULL REFERENCES accounts(id),
    updated_by_account_id UUID NOT NULL REFERENCES accounts(id),
    deleted_by_account_id UUID REFERENCES accounts(id),
    purge_after TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,

    UNIQUE (id, space_id),
    FOREIGN KEY (parent_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,

    CHECK (
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND deleted_at IS NULL)
        OR (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (parent_id IS NULL OR name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$'),
    CHECK (name NOT IN ('.', '..')),
    CHECK (jsonb_typeof(metadata) = 'object'),
    CHECK (
        (deleted_at IS NULL AND deleted_by_account_id IS NULL AND purge_after IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by_account_id IS NOT NULL AND purge_after IS NOT NULL)
    )
);
CREATE INDEX nodes_purge_due_idx
    ON nodes(purge_after, space_id, id)
    WHERE deleted_at IS NOT NULL;
CREATE UNIQUE INDEX nodes_one_root_per_space
    ON nodes(space_id)
    WHERE parent_id IS NULL;
CREATE UNIQUE INDEX nodes_live_sibling_name_key
    ON nodes(space_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
CREATE INDEX nodes_children_idx
    ON nodes(space_id, parent_id, sort_order, name, id)
    WHERE deleted_at IS NULL;
CREATE INDEX nodes_kind_idx
    ON nodes(space_id, kind)
    WHERE deleted_at IS NULL;

CREATE OR REPLACE FUNCTION create_space_root_node()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id)
    VALUES (NEW.id, NULL, '/', 'folder', NEW.owner_user_id, NEW.owner_user_id);
    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_create_root_node
AFTER INSERT ON spaces
FOR EACH ROW
EXECUTE FUNCTION create_space_root_node();

CREATE TABLE text_objects (
    node_id UUID PRIMARY KEY,
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    storage_format TEXT NOT NULL DEFAULT 'plain' CHECK (storage_format IN ('plain', 'encrypted')),
    content_text TEXT,
    encrypted_payload JSONB,
    content_sha256 TEXT NOT NULL DEFAULT '',
    byte_len BIGINT NOT NULL DEFAULT 0,
    line_count INTEGER NOT NULL DEFAULT 0,
    media_type TEXT NOT NULL DEFAULT 'text/plain',
    encoding TEXT NOT NULL DEFAULT 'utf-8',
    created_by_account_id UUID NOT NULL REFERENCES accounts(id),
    updated_by_account_id UUID NOT NULL REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (node_id, space_id),
    FOREIGN KEY (node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0 AND byte_len <= 1048576),
    CHECK (line_count >= 0 AND line_count <= 2000),
    CHECK (encoding = 'utf-8'),
    CHECK (
        (storage_format = 'plain' AND content_text IS NOT NULL AND encrypted_payload IS NULL)
        OR (storage_format = 'encrypted' AND content_text IS NULL AND encrypted_payload IS NOT NULL AND jsonb_typeof(encrypted_payload) = 'object')
    )
);
CREATE INDEX text_objects_space_idx ON text_objects(space_id);
CREATE INDEX text_objects_space_updated_idx ON text_objects(space_id, updated_at DESC, node_id);

CREATE TABLE file_objects (
    node_id UUID PRIMARY KEY,
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    storage_kind TEXT NOT NULL CHECK (storage_kind IN ('inline_pg', 'object')),
    object_key TEXT,
    media_type TEXT NOT NULL,
    byte_len BIGINT NOT NULL,
    content_sha256 TEXT NOT NULL,
    original_filename TEXT,
    encryption_mode TEXT NOT NULL DEFAULT 'none' CHECK (encryption_mode IN ('none', 'client')),
    encryption_metadata JSONB,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (node_id, space_id),
    FOREIGN KEY (node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0 AND byte_len <= 104857600),
    CHECK (
        (storage_kind = 'inline_pg' AND object_key IS NULL AND byte_len <= 262144)
        OR (storage_kind = 'object' AND object_key IS NOT NULL)
    ),
    CHECK (
        (encryption_mode = 'none' AND encryption_metadata IS NULL)
        OR (encryption_mode = 'client' AND encryption_metadata IS NOT NULL AND jsonb_typeof(encryption_metadata) = 'object')
    )
);
CREATE INDEX file_objects_space_idx ON file_objects(space_id);

CREATE TABLE file_inline_contents (
    node_id UUID PRIMARY KEY,
    space_id UUID NOT NULL,
    bytes BYTEA NOT NULL,

    FOREIGN KEY (node_id, space_id)
        REFERENCES file_objects(node_id, space_id)
        ON DELETE CASCADE,
    CHECK (octet_length(bytes) <= 262144)
);

CREATE OR REPLACE FUNCTION assert_node_content_kind()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    node_kind TEXT;
BEGIN
    SELECT kind INTO node_kind FROM nodes WHERE id = NEW.node_id AND space_id = NEW.space_id;
    IF TG_TABLE_NAME = 'text_objects' AND node_kind <> 'text' THEN
        RAISE EXCEPTION 'text_objects row requires nodes.kind=text';
    END IF;
    IF TG_TABLE_NAME = 'file_objects' AND node_kind <> 'file' THEN
        RAISE EXCEPTION 'file_objects row requires nodes.kind=file';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER text_objects_kind_check
BEFORE INSERT OR UPDATE ON text_objects
FOR EACH ROW EXECUTE FUNCTION assert_node_content_kind();

CREATE TRIGGER file_objects_kind_check
BEFORE INSERT OR UPDATE ON file_objects
FOR EACH ROW EXECUTE FUNCTION assert_node_content_kind();
