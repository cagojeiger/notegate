-- notegate schema: tree nodes, text content, and file content.

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
CREATE UNIQUE INDEX nodes_one_root_per_space
    ON nodes(space_id)
    WHERE parent_id IS NULL;
CREATE UNIQUE INDEX nodes_live_sibling_name_key
    ON nodes(space_id, parent_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
CREATE INDEX nodes_children_idx
    ON nodes(space_id, parent_id, sort_order, name, id)
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
