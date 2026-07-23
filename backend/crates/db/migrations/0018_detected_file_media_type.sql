-- Provider bytes, not client declarations, determine whether a file can be
-- rendered inline.

ALTER TABLE file_objects
    ADD COLUMN detected_media_type TEXT,
    ADD CONSTRAINT file_objects_detected_media_type_encryption_check CHECK (
        encryption_mode = 'none' OR detected_media_type IS NULL
    );
