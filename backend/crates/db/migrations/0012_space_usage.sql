-- Authoritative counters for Space-scoped node and content usage.

CREATE TABLE space_usage (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    live_node_count BIGINT NOT NULL DEFAULT 1 CHECK (live_node_count >= 1),
    live_content_bytes BIGINT NOT NULL DEFAULT 0 CHECK (live_content_bytes >= 0),
    reconciled_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX agents_owner_user_idx ON agents(owner_user_id);

CREATE TABLE space_usage_reconcile_jobs (
    job_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id UUID NOT NULL UNIQUE REFERENCES spaces(id) ON DELETE CASCADE,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0)
);
CREATE INDEX space_usage_reconcile_jobs_ready_idx
    ON space_usage_reconcile_jobs(run_after, requested_at, job_id);

CREATE TABLE space_usage_reconcile_executions (
    execution_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id UUID NOT NULL,
    space_id UUID NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    outcome TEXT NOT NULL CHECK (outcome IN ('succeeded', 'deferred', 'failed', 'cancelled')),
    error_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    CHECK ((outcome = 'failed') = (error_message IS NOT NULL))
);
CREATE INDEX space_usage_reconcile_executions_space_idx
    ON space_usage_reconcile_executions(space_id, started_at DESC, execution_id DESC);
CREATE INDEX space_usage_reconcile_executions_retention_idx
    ON space_usage_reconcile_executions(finished_at);

INSERT INTO space_usage (
    space_id,
    live_node_count,
    live_content_bytes,
    reconciled_at
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
    now()
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
