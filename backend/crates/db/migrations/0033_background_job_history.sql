ALTER TABLE background_jobs
    ADD COLUMN history_visibility TEXT NOT NULL DEFAULT 'hidden'
        CHECK (history_visibility IN ('hidden', 'visible')),
    ADD COLUMN history_owner_account_id UUID,
    ADD COLUMN context_kind TEXT,
    ADD COLUMN context_id UUID,
    ADD COLUMN context_label TEXT,
    ADD CHECK (
        (history_visibility = 'hidden' AND history_owner_account_id IS NULL)
        OR (history_visibility = 'visible' AND history_owner_account_id IS NOT NULL)
    ),
    ADD CHECK (context_kind IS NULL OR octet_length(context_kind) BETWEEN 1 AND 64),
    ADD CHECK (context_label IS NULL OR octet_length(context_label) BETWEEN 1 AND 256);

-- Preserve rollout continuity for active usage jobs created before the common
-- queue envelope carried history metadata. Terminal legacy rows remain hidden:
-- their owning Space may be purged before the queue's longer retention expires,
-- so ownership cannot be reconstructed durably for the full history window.
UPDATE background_jobs job
SET history_visibility = 'visible',
    history_owner_account_id = space.owner_user_id,
    context_kind = 'space',
    context_id = space.id,
    context_label = space.name
FROM spaces space
WHERE job.job_kind = 'space_usage_reconcile'
  AND job.status IN ('queued', 'running')
  AND job.payload ->> 'space_id' = space.id::text;

CREATE INDEX background_jobs_owner_history_idx
    ON background_jobs (history_owner_account_id, created_at DESC, job_id DESC)
    WHERE history_visibility = 'visible';

-- New callers use the history-aware overload.
CREATE FUNCTION enqueue_background_job(
    TEXT, JSONB, TIMESTAMPTZ, INTEGER, TEXT, UUID, TEXT, UUID, TEXT
)
RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    queued_job_id UUID;
BEGIN
    INSERT INTO background_jobs (
        job_kind, payload, available_at, max_attempts,
        history_visibility, history_owner_account_id,
        context_kind, context_id, context_label
    )
    VALUES ($1, $2, COALESCE($3, now()), $4, $5, $6, $7, $8, $9)
    RETURNING job_id INTO queued_job_id;

    PERFORM pg_notify('notegate_background_jobs', $1);
    RETURN queued_job_id;
END;
$$;

-- Replicas from the preceding release still call the four-argument function.
-- Preserve their usage-job history during a rolling deployment while keeping
-- every other legacy job hidden by default.
CREATE OR REPLACE FUNCTION enqueue_background_job(TEXT, JSONB, TIMESTAMPTZ, INTEGER)
RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    job_owner_account_id UUID;
    job_context_id UUID;
    job_context_label TEXT;
BEGIN
    IF $1 = 'space_usage_reconcile' THEN
        SELECT owner_user_id, id, name
        INTO job_owner_account_id, job_context_id, job_context_label
        FROM spaces
        WHERE id::text = ($2 ->> 'space_id');
    END IF;

    IF job_owner_account_id IS NOT NULL THEN
        RETURN enqueue_background_job(
            $1, $2, $3, $4,
            'visible', job_owner_account_id,
            'space', job_context_id, job_context_label
        );
    END IF;

    RETURN enqueue_background_job(
        $1, $2, $3, $4,
        'hidden', NULL,
        NULL, NULL, NULL
    );
END;
$$;

-- Old API replicas still write the compatibility queue. Mirror those jobs
-- with the same history metadata as new API replicas.
CREATE OR REPLACE FUNCTION mirror_legacy_space_usage_job()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    job_owner_account_id UUID;
    job_context_label TEXT;
BEGIN
    SELECT owner_user_id, name
    INTO job_owner_account_id, job_context_label
    FROM spaces
    WHERE id = NEW.space_id;

    PERFORM enqueue_background_job(
        'space_usage_reconcile',
        jsonb_build_object('space_id', NEW.space_id),
        NEW.run_after,
        8,
        'visible',
        job_owner_account_id,
        'space',
        NEW.space_id,
        job_context_label
    );
    RETURN NEW;
END;
$$;
