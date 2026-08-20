CREATE FUNCTION ensure_link_graph_job_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    owner_account_id UUID;
    space_id UUID;
    space_name TEXT;
BEGIN
    SELECT space.owner_user_id, space.id, space.name
    INTO owner_account_id, space_id, space_name
    FROM spaces space
    WHERE space.id::text = NEW.payload ->> 'space_id';

    IF owner_account_id IS NOT NULL THEN
        NEW.history_visibility = 'visible';
        NEW.history_owner_account_id = owner_account_id;
        NEW.context_kind = 'space';
        NEW.context_id = space_id;
        NEW.context_label = space_name;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER background_jobs_ensure_link_history
BEFORE INSERT ON background_jobs
FOR EACH ROW
WHEN (
    NEW.job_kind = 'link_graph_project_nodes'
    AND NEW.history_visibility = 'hidden'
)
EXECUTE FUNCTION ensure_link_graph_job_history();

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
