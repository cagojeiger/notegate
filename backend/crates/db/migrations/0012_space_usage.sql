-- Authoritative counters for Space-scoped node and content usage.

CREATE TABLE space_usage (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    live_node_count BIGINT NOT NULL DEFAULT 1 CHECK (live_node_count >= 1),
    live_content_bytes BIGINT NOT NULL DEFAULT 0 CHECK (live_content_bytes >= 0),
    reconciled_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    next_reconcile_at TIMESTAMPTZ NOT NULL DEFAULT now() + (random() * interval '7 days')
);
CREATE INDEX space_usage_reconcile_due_idx ON space_usage(next_reconcile_at);
CREATE INDEX agents_owner_user_idx ON agents(owner_user_id);

INSERT INTO space_usage (
    space_id,
    live_node_count,
    live_content_bytes,
    reconciled_at,
    next_reconcile_at
)
SELECT
    s.id,
    (SELECT count(*) FROM nodes n WHERE n.space_id = s.id AND n.deleted_at IS NULL),
    COALESCE((
        SELECT sum(t.byte_len)
        FROM text_objects t
        JOIN nodes n ON n.id = t.node_id AND n.space_id = t.space_id
        WHERE t.space_id = s.id AND n.deleted_at IS NULL
    ), 0) + COALESCE((
        SELECT sum(f.byte_len)
        FROM file_objects f
        JOIN nodes n ON n.id = f.node_id AND n.space_id = f.space_id
        WHERE f.space_id = s.id AND n.deleted_at IS NULL
    ), 0),
    now(),
    now() + (random() * interval '7 days')
FROM spaces s;

CREATE OR REPLACE FUNCTION create_space_usage()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO space_usage (space_id) VALUES (NEW.id);
    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_create_usage
AFTER INSERT ON spaces
FOR EACH ROW
EXECUTE FUNCTION create_space_usage();
