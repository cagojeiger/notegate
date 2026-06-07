-- Initial schema.
-- `users` is notegate's identity foundation: every authenticated caller
-- (browser, REST, MCP) resolves to exactly one row here.

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE IF NOT EXISTS users (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sub          TEXT NOT NULL UNIQUE,
    email        TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    is_active    BOOLEAN NOT NULL DEFAULT true,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS users_email_idx
    ON users(email);

CREATE TABLE IF NOT EXISTS workspaces (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (owner_user_id, name)
);

CREATE TABLE IF NOT EXISTS nodes (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id     UUID,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('folder', 'document')),
    path_cache    TEXT NOT NULL,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,

    UNIQUE (id, workspace_id),
    FOREIGN KEY (parent_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,

    CHECK (
        (parent_id IS NULL AND name = '/' AND kind = 'folder' AND path_cache = '/')
        OR
        (parent_id IS NOT NULL AND name <> '' AND name NOT LIKE '%/%')
    ),
    CHECK (path_cache LIKE '/%'),
    CHECK (kind <> 'document' OR name LIKE '%.md'),
    CHECK (kind <> 'folder' OR parent_id IS NULL OR name NOT LIKE '%.md')
);

CREATE UNIQUE INDEX IF NOT EXISTS nodes_one_root_per_workspace
    ON nodes(workspace_id)
    WHERE parent_id IS NULL AND deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_live_sibling_name_key
    ON nodes(workspace_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_live_path_key
    ON nodes(workspace_id, path_cache)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_children_idx
    ON nodes(workspace_id, parent_id, sort_order, name, id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_path_trgm_idx
    ON nodes USING gin (path_cache gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS documents (
    node_id        UUID PRIMARY KEY,
    workspace_id   UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md     TEXT NOT NULL DEFAULT '',
    content_sha256 TEXT NOT NULL DEFAULT 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    byte_len       INTEGER NOT NULL DEFAULT 0,
    line_count     INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (node_id, workspace_id),
    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE,
    CHECK (byte_len >= 0),
    CHECK (line_count >= 0)
);

CREATE INDEX IF NOT EXISTS documents_content_trgm_idx
    ON documents USING gin (content_md gin_trgm_ops);

CREATE INDEX IF NOT EXISTS documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC, node_id);

CREATE TABLE IF NOT EXISTS document_lines (
    workspace_id UUID NOT NULL,
    node_id      UUID NOT NULL,
    line_no      INTEGER NOT NULL,
    line_text    TEXT NOT NULL,
    line_hash    TEXT NOT NULL DEFAULT '',

    PRIMARY KEY (node_id, line_no),
    FOREIGN KEY (node_id, workspace_id)
        REFERENCES documents(node_id, workspace_id)
        ON DELETE CASCADE,
    CHECK (line_no >= 1)
);

CREATE INDEX IF NOT EXISTS document_lines_workspace_text_trgm_idx
    ON document_lines USING gin (line_text gin_trgm_ops);

CREATE INDEX IF NOT EXISTS document_lines_workspace_node_line_idx
    ON document_lines(workspace_id, node_id, line_no);

CREATE TABLE IF NOT EXISTS document_index_status (
    node_id         UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL,
    content_sha256  TEXT NOT NULL,
    index_version   INTEGER NOT NULL DEFAULT 1,
    status          TEXT NOT NULL CHECK (status IN ('ready', 'stale', 'failed')),
    error           TEXT,
    indexed_at      TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (node_id, workspace_id)
        REFERENCES documents(node_id, workspace_id)
        ON DELETE CASCADE
);
