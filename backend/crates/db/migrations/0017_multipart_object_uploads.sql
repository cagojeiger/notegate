-- Large object files use S3 multipart uploads while retaining the existing
-- Notegate upload ledger and cleanup lifecycle.

ALTER TABLE file_objects
    DROP CONSTRAINT file_objects_byte_len_check,
    ADD CONSTRAINT file_objects_byte_len_check
        CHECK (byte_len >= 0 AND byte_len <= 107374182400);

ALTER TABLE object_storage_objects
    DROP CONSTRAINT object_storage_objects_declared_byte_len_check,
    ADD CONSTRAINT object_storage_objects_declared_byte_len_check
        CHECK (declared_byte_len BETWEEN 0 AND 107374182400),
    ADD COLUMN upload_mode TEXT NOT NULL DEFAULT 'single'
        CHECK (upload_mode IN ('single', 'multipart')),
    ADD COLUMN multipart_upload_id TEXT,
    ADD COLUMN multipart_part_size BIGINT,
    ADD CONSTRAINT object_storage_objects_multipart_shape_check CHECK (
        (upload_mode = 'single'
            AND multipart_upload_id IS NULL
            AND multipart_part_size IS NULL)
        OR
        (upload_mode = 'multipart'
            AND multipart_upload_id IS NOT NULL
            AND multipart_part_size > 0)
    );
