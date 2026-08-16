DROP TRIGGER space_usage_jobs_mirror_background_queue ON space_usage_reconcile_jobs;
DROP FUNCTION mirror_legacy_space_usage_job();
DROP FUNCTION try_lock_background_job_reconciler();
DROP FUNCTION enqueue_background_job(TEXT, JSONB, TIMESTAMPTZ, INTEGER);

DROP TABLE space_usage_reconcile_executions;
DROP TABLE space_usage_reconcile_jobs;
