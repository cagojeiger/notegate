-- File content is stored exclusively in S3-compatible object storage.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM file_objects WHERE storage_kind = 'inline_pg') THEN
        RAISE EXCEPTION 'inline files must be purged before applying 0015';
    END IF;
END $$;

DROP TABLE file_inline_contents;

ALTER TABLE file_objects
    DROP CONSTRAINT file_objects_content_hash_check,
    DROP CONSTRAINT file_objects_check,
    DROP CONSTRAINT file_objects_storage_kind_check,
    DROP COLUMN storage_kind,
    DROP COLUMN content_sha256,
    ALTER COLUMN object_key SET NOT NULL;
