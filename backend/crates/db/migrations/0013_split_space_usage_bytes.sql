-- Split Space content usage so Text and File capacity can evolve independently.

ALTER TABLE space_usage
    ADD COLUMN live_text_bytes BIGINT NOT NULL DEFAULT 0 CHECK (live_text_bytes >= 0),
    ADD COLUMN live_file_bytes BIGINT NOT NULL DEFAULT 0 CHECK (live_file_bytes >= 0);

UPDATE space_usage su
SET live_text_bytes = COALESCE((
        SELECT sum(t.byte_len)
        FROM text_objects t
        JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id
        WHERE t.space_id = su.space_id AND n.deleted_at IS NULL
    ), 0),
    live_file_bytes = COALESCE((
        SELECT sum(f.byte_len)
        FROM file_objects f
        JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id
        WHERE f.space_id = su.space_id AND n.deleted_at IS NULL
    ), 0),
    reconciled_at = now();

ALTER TABLE space_usage DROP COLUMN live_content_bytes;
