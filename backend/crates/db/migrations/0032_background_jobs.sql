CREATE TABLE background_jobs (
    job_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'succeeded', 'dead')),
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 8 CHECK (max_attempts BETWEEN 1 AND 100),
    claim_token UUID,
    claimed_by TEXT,
    lease_until TIMESTAMPTZ,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CHECK (octet_length(job_kind) BETWEEN 1 AND 128),
    CHECK (octet_length(payload::text) <= 65536),
    CHECK (failure_count <= max_attempts),
    CHECK (claimed_by IS NULL OR octet_length(claimed_by) BETWEEN 1 AND 256),
    CHECK (last_error_code IS NULL OR octet_length(last_error_code) BETWEEN 1 AND 128),
    CHECK (last_error_message IS NULL OR octet_length(last_error_message) <= 4096),
    CHECK (
        (status = 'queued'
            AND claim_token IS NULL AND claimed_by IS NULL AND lease_until IS NULL
            AND completed_at IS NULL AND failure_count < max_attempts)
        OR
        (status = 'running'
            AND claim_token IS NOT NULL AND claimed_by IS NOT NULL AND lease_until IS NOT NULL
            AND completed_at IS NULL)
        OR
        (status IN ('succeeded', 'dead')
            AND claim_token IS NULL AND claimed_by IS NULL AND lease_until IS NULL
            AND completed_at IS NOT NULL)
    )
);

CREATE INDEX background_jobs_ready_idx
    ON background_jobs (available_at, created_at, job_id)
    WHERE status = 'queued';
CREATE INDEX background_jobs_expired_lease_idx
    ON background_jobs (lease_until, job_id)
    WHERE status = 'running';
CREATE INDEX background_jobs_kind_state_idx
    ON background_jobs (job_kind, status, created_at, job_id)
    WHERE status IN ('queued', 'running', 'dead');
CREATE INDEX background_jobs_retention_idx
    ON background_jobs (completed_at, job_id)
    WHERE status IN ('succeeded', 'dead');

CREATE TABLE background_job_attempts (
    job_id UUID NOT NULL REFERENCES background_jobs(job_id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    claim_token UUID NOT NULL,
    worker_id TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    outcome TEXT CHECK (
        outcome IN (
            'succeeded', 'retryable_error', 'permanent_error', 'timed_out',
            'panicked', 'cancelled', 'lease_expired', 'deferred'
        )
    ),
    error_code TEXT,
    error_message TEXT,
    PRIMARY KEY (job_id, attempt_number),
    UNIQUE (job_id, claim_token),
    CHECK (octet_length(worker_id) BETWEEN 1 AND 256),
    CHECK (error_code IS NULL OR octet_length(error_code) BETWEEN 1 AND 128),
    CHECK (error_message IS NULL OR octet_length(error_message) <= 4096),
    CHECK (
        (finished_at IS NULL AND outcome IS NULL AND error_code IS NULL AND error_message IS NULL)
        OR (finished_at IS NOT NULL AND outcome IS NOT NULL)
    )
);

CREATE UNIQUE INDEX background_job_attempt_one_open_idx
    ON background_job_attempts (job_id)
    WHERE finished_at IS NULL;

CREATE FUNCTION enqueue_background_job(TEXT, JSONB, TIMESTAMPTZ, INTEGER)
RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    queued_job_id UUID;
BEGIN
    INSERT INTO background_jobs (job_kind, payload, available_at, max_attempts)
    VALUES ($1, $2, COALESCE($3, now()), $4)
    RETURNING job_id INTO queued_job_id;

    PERFORM pg_notify('notegate_background_jobs', $1);
    RETURN queued_job_id;
END;
$$;

CREATE FUNCTION background_job_backlog(TEXT DEFAULT NULL)
RETURNS BIGINT
LANGUAGE SQL
STABLE
AS $$
    SELECT count(*)::BIGINT
    FROM background_jobs
    WHERE ($1 IS NULL OR job_kind = $1)
      AND (
          (status = 'queued' AND available_at <= now())
          OR status = 'running'
      );
$$;

CREATE FUNCTION try_lock_background_job_reconciler()
RETURNS BOOLEAN
LANGUAGE SQL
VOLATILE
AS $$
    SELECT pg_try_advisory_xact_lock(5640558762580443137);
$$;

-- Preserve pending usage work during a rolling deployment. Old replicas can
-- still insert into the legacy table until they leave service; the insert
-- trigger mirrors those requests into the new durable queue.
LOCK TABLE space_usage_reconcile_jobs IN SHARE ROW EXCLUSIVE MODE;

INSERT INTO background_jobs (
    job_kind, payload, available_at, attempt_count, failure_count, max_attempts,
    created_at, updated_at
)
SELECT
    'space_usage_reconcile',
    jsonb_build_object('space_id', space_id),
    run_after,
    LEAST(retry_count, 7),
    LEAST(retry_count, 7),
    8,
    requested_at,
    now()
FROM space_usage_reconcile_jobs;

CREATE FUNCTION mirror_legacy_space_usage_job()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM enqueue_background_job(
        'space_usage_reconcile',
        jsonb_build_object('space_id', NEW.space_id),
        NEW.run_after,
        8
    );
    RETURN NEW;
END;
$$;

CREATE TRIGGER space_usage_jobs_mirror_background_queue
AFTER INSERT ON space_usage_reconcile_jobs
FOR EACH ROW
EXECUTE FUNCTION mirror_legacy_space_usage_job();

CREATE INDEX background_jobs_usage_active_idx
    ON background_jobs ((payload ->> 'space_id'), status, created_at, job_id)
    WHERE job_kind = 'space_usage_reconcile'
      AND status IN ('queued', 'running');
