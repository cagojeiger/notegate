CREATE EXTENSION IF NOT EXISTS pg_trgm;

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
    WHERE parent_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_live_workspace_name_key
    ON nodes(workspace_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_live_path_key
    ON nodes(workspace_id, path_cache)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_children_idx
    ON nodes(workspace_id, parent_id, sort_order, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS nodes_path_trgm_idx
    ON nodes USING gin (path_cache gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS documents (
    node_id       UUID PRIMARY KEY,
    workspace_id  UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    content_md    TEXT NOT NULL DEFAULT '',
    search_text   TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (node_id, workspace_id)
        REFERENCES nodes(id, workspace_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS documents_search_text_trgm_idx
    ON documents USING gin (search_text gin_trgm_ops);
