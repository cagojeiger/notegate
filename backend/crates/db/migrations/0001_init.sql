-- Initial schema (account/workspace/access model).
--
-- `accounts` is the common actor identity: every authenticated caller (user or
-- agent) resolves to exactly one row. `users` and `agents` carry the
-- kind-specific detail. Nodes have NO stored path column — the canonical
-- location is `parent_id + name` and display paths are derived, so
-- folder move/rename stays O(1).

CREATE EXTENSION IF NOT EXISTS pgcrypto;   -- gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- accounts: common actor identity.
CREATE TABLE accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind TEXT NOT NULL CHECK (kind IN ('user', 'agent')),
    display_name_ciphertext BYTEA,
    display_name_nonce BYTEA,
    is_active BOOLEAN NOT NULL DEFAULT true,
    deleted_at TIMESTAMPTZ,
    deleted_by UUID REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (display_name_ciphertext IS NULL AND display_name_nonce IS NULL)
        OR (display_name_ciphertext IS NOT NULL AND display_name_nonce IS NOT NULL)
    ),
    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by IS NOT NULL)
    )
);
-- NOTE: accounts has NO created_by (self-registration; attribution target only).
-- User display names are encrypted in accounts; agent display names are derived
-- from agents.name, so accounts never stores plaintext user PII.

-- account_encryption_keys: one encrypted DEK per user account.
CREATE TABLE account_encryption_keys (
    account_id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    kek_id TEXT NOT NULL,
    kek_version TEXT,
    wrapped_dek BYTEA,
    algorithm TEXT NOT NULL DEFAULT 'AES-256-GCM',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    rewrapped_at TIMESTAMPTZ,
    destroyed_at TIMESTAMPTZ,
    CHECK (
        (destroyed_at IS NULL AND wrapped_dek IS NOT NULL)
        OR (destroyed_at IS NOT NULL AND wrapped_dek IS NULL)
    )
);

-- users: OAuth detail; id = accounts.id. No plaintext subject/email is stored.
CREATE TABLE users (
    id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    provider_sub_hash TEXT UNIQUE,
    provider_sub_hash_version INTEGER NOT NULL DEFAULT 1,
    email_ciphertext BYTEA,
    email_nonce BYTEA,
    email_hash TEXT,
    email_hash_version INTEGER,
    anonymized_at TIMESTAMPTZ,
    CHECK (
        (email_ciphertext IS NULL AND email_nonce IS NULL)
        OR (email_ciphertext IS NOT NULL AND email_nonce IS NOT NULL)
    )
);
CREATE INDEX users_email_hash_idx ON users(email_hash) WHERE email_hash IS NOT NULL;

-- agents: machine-actor detail; id = accounts.id. Agent name is non-PII product metadata.
CREATE TABLE agents (
    id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES accounts(id)
);

-- agent_keys: bearer keys authenticating as an agent (hash only, never plaintext).
CREATE TABLE agent_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[] CHECK (cardinality(scopes) = 0),
    created_by UUID REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_by UUID REFERENCES accounts(id)
);
CREATE INDEX agent_keys_agent_active_idx ON agent_keys(agent_id) WHERE revoked_at IS NULL;

-- workspaces: a named tree whose lifecycle owner is the creator user.
CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    deleted_by UUID REFERENCES accounts(id),
    purge_after TIMESTAMPTZ,
    CHECK (name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$'),
    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by IS NOT NULL AND purge_after IS NOT NULL)
    )
);

CREATE UNIQUE INDEX workspaces_created_by_name_live_uidx
    ON workspaces(created_by, name)
    WHERE deleted_at IS NULL;

-- workspace_access: per-account owner/editor/viewer membership grants.
CREATE TABLE workspace_access (
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'viewer', 'editor')),
    granted_by UUID REFERENCES accounts(id),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    revoked_by UUID REFERENCES accounts(id),
    PRIMARY KEY (workspace_id, account_id)
);
CREATE INDEX workspace_access_account_idx ON workspace_access(account_id) WHERE revoked_at IS NULL;
CREATE INDEX workspace_access_owner_active_idx
    ON workspace_access(workspace_id, account_id)
    WHERE revoked_at IS NULL AND role = 'owner';

-- nodes: the canonical tree (parent_id + name). NO stored path.
CREATE TABLE nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id UUID,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('folder', 'document')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_by UUID NOT NULL REFERENCES accounts(id),
    updated_by UUID NOT NULL REFERENCES accounts(id),
    deleted_by UUID REFERENCES accounts(id),
    purge_after TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,

    UNIQUE (id, workspace_id),
    FOREIGN KEY (parent_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,

    CHECK (
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND deleted_at IS NULL)
        OR (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (parent_id IS NULL OR name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$'),
    CHECK (name NOT IN ('.', '..')),
    CHECK (kind <> 'document' OR name LIKE '%.md'),
    CHECK (kind <> 'folder' OR parent_id IS NULL OR name NOT LIKE '%.md'),
    CHECK (
        (deleted_at IS NULL AND deleted_by IS NULL AND purge_after IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by IS NOT NULL AND purge_after IS NOT NULL)
    )
);

CREATE INDEX nodes_purge_due_idx
    ON nodes(purge_after, workspace_id, id)
    WHERE deleted_at IS NOT NULL;

CREATE UNIQUE INDEX nodes_one_root_per_workspace
    ON nodes(workspace_id)
    WHERE parent_id IS NULL;

CREATE UNIQUE INDEX nodes_live_sibling_name_key
    ON nodes(workspace_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;

CREATE INDEX nodes_children_idx
    ON nodes(workspace_id, parent_id, sort_order, name, id)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;
-- NO path indexes. Scope/display paths use workspace-bounded recursive CTEs
-- (depth <= 5, nodes <= 10000).

-- Root trigger: sets attribution (created_by/updated_by) from the workspace creator.
CREATE OR REPLACE FUNCTION create_workspace_root_node()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO nodes (workspace_id, parent_id, name, kind, created_by, updated_by)
    VALUES (NEW.id, NULL, '/', 'folder', NEW.created_by, NEW.created_by);
    RETURN NEW;
END;
$$;

CREATE TRIGGER workspaces_create_root_node
AFTER INSERT ON workspaces
FOR EACH ROW
EXECUTE FUNCTION create_workspace_root_node();

-- documents: content keyed to a node, with attribution and tightened bounds.
CREATE TABLE documents (
    node_id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md TEXT NOT NULL DEFAULT '',
    content_sha256 TEXT NOT NULL DEFAULT '',
    byte_len INTEGER NOT NULL DEFAULT 0,
    line_count INTEGER NOT NULL DEFAULT 0,
    created_by UUID NOT NULL REFERENCES accounts(id),
    updated_by UUID NOT NULL REFERENCES accounts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (node_id, workspace_id),
    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0 AND byte_len <= 524288),
    CHECK (line_count >= 0 AND line_count <= 2000)
);

CREATE INDEX documents_content_trgm_idx
    ON documents USING gin (content_md gin_trgm_ops);
-- grep orders by updated_at DESC: keep this index for the grep keyset cursor.
CREATE INDEX documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC, node_id);
