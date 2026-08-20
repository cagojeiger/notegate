pub(crate) const NODE_REQUEST_PENDING_PREDICATE: &str = "projection.needs_projection \
    AND ( \
        (projection.active_job_id IS NULL AND projection.failed_at IS NULL) \
        OR job.status IN ('queued', 'running', 'succeeded') \
        OR ( \
            job.status = 'dead' \
            AND projection.active_request_version IS DISTINCT FROM projection.request_version \
        ) \
    )";
