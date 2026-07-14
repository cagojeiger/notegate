-- S3-compatible object files retain their opaque object key. The operational
-- ledger survives node/space deletion so physical cleanup can be retried after
-- semantic rows have been purged.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM file_objects WHERE storage_kind = 'object') THEN
        RAISE EXCEPTION 'legacy object-backed files must be migrated before applying 0014';
    END IF;
END $$;

ALTER TABLE file_objects
    ALTER COLUMN content_sha256 DROP NOT NULL;

ALTER TABLE file_objects
    ADD CONSTRAINT file_objects_content_hash_check CHECK (
        storage_kind = 'object' OR content_sha256 IS NOT NULL
    );

CREATE UNIQUE INDEX file_objects_object_key_idx
    ON file_objects(object_key)
    WHERE object_key IS NOT NULL;

CREATE TABLE object_storage_objects (
    id UUID PRIMARY KEY,
    object_key TEXT NOT NULL UNIQUE,
    space_id UUID REFERENCES spaces(id) ON DELETE SET NULL,
    parent_node_id UUID REFERENCES nodes(id) ON DELETE SET NULL,
    node_id UUID UNIQUE REFERENCES nodes(id) ON DELETE SET NULL,
    requested_by_account_id UUID REFERENCES accounts(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    declared_byte_len BIGINT NOT NULL CHECK (declared_byte_len BETWEEN 0 AND 104857600),
    media_type TEXT NOT NULL,
    original_filename TEXT,
    encryption_mode TEXT NOT NULL DEFAULT 'none' CHECK (encryption_mode IN ('none','client')),
    encryption_metadata JSONB,
    state TEXT NOT NULL CHECK (state IN (
        'uploading','attached','expire_pending','expired','delete_pending','deleted'
    )),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    retry_after TIMESTAMPTZ,
    last_error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    attached_at TIMESTAMPTZ,
    delete_requested_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    CHECK (
        (encryption_mode = 'none' AND encryption_metadata IS NULL)
        OR
        (encryption_mode = 'client' AND encryption_metadata IS NOT NULL
            AND jsonb_typeof(encryption_metadata) = 'object')
    ),
    CHECK (
        (state = 'attached' AND node_id IS NOT NULL AND attached_at IS NOT NULL)
        OR state <> 'attached'
    )
);

CREATE INDEX object_storage_objects_cleanup_idx
    ON object_storage_objects(state, retry_after, last_activity_at, id)
    WHERE state IN ('uploading','expire_pending','delete_pending');

ALTER TABLE file_objects
    ADD CONSTRAINT file_objects_object_key_fk
    FOREIGN KEY (object_key)
    REFERENCES object_storage_objects(object_key);
