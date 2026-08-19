-- Link projection jobs predate the generic account-scoped history envelope.
-- Backfill their durable owner and display context once so history reads remain
-- generic and do not need to decode a domain-specific payload.
UPDATE background_jobs job
SET history_visibility = 'visible',
    history_owner_account_id = space.owner_user_id,
    context_kind = 'space',
    context_id = space.id,
    context_label = space.name
FROM spaces space
WHERE job.job_kind = 'link_graph_project_nodes'
  AND job.history_visibility = 'hidden'
  AND job.payload ->> 'space_id' = space.id::text;
