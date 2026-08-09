CREATE TABLE node_link_refs (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL,
    target_node_id UUID,
    target_path TEXT NOT NULL,
    reference_kind TEXT NOT NULL CHECK (reference_kind IN ('link', 'image')),
    occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
    PRIMARY KEY (space_id, source_node_id, reference_kind, target_path),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    FOREIGN KEY (target_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE SET NULL (target_node_id),
    CHECK (target_path LIKE '/%' AND octet_length(target_path) <= 903)
);

CREATE INDEX node_link_refs_incoming_idx
    ON node_link_refs (space_id, target_node_id, source_node_id, reference_kind)
    WHERE target_node_id IS NOT NULL;

CREATE INDEX node_link_refs_unresolved_path_idx
    ON node_link_refs (space_id, target_path, source_node_id)
    WHERE target_node_id IS NULL;

CREATE TABLE node_link_source_states (
    space_id UUID NOT NULL,
    source_node_id UUID NOT NULL,
    source_content_sha256 TEXT NOT NULL,
    source_path TEXT NOT NULL,
    parser_version INTEGER NOT NULL CHECK (parser_version > 0),
    last_synced_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (space_id, source_node_id),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (source_path LIKE '/%' AND octet_length(source_path) <= 903)
);

CREATE TABLE node_link_space_states (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    last_synced_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX background_jobs_link_source_status_idx
    ON background_jobs (
        (payload ->> 'space_id'), (payload ->> 'source_node_id'), status, created_at, job_id
    )
    WHERE job_kind = 'node_link_source'
      AND status IN ('queued', 'running', 'dead');
CREATE UNIQUE INDEX background_jobs_link_source_fresh_uidx
    ON background_jobs (
        (payload ->> 'space_id'), (payload ->> 'source_node_id')
    )
    WHERE job_kind = 'node_link_source'
      AND status = 'queued'
      AND failure_count = 0;
CREATE UNIQUE INDEX background_jobs_link_impact_fresh_uidx
    ON background_jobs (
        (payload ->> 'space_id'), (payload ->> 'changed_node_id')
    )
    WHERE job_kind = 'node_link_impact'
      AND status = 'queued'
      AND failure_count = 0;
CREATE UNIQUE INDEX background_jobs_link_space_fresh_uidx
    ON background_jobs ((payload ->> 'space_id'))
    WHERE job_kind = 'node_link_space'
      AND status = 'queued'
      AND failure_count = 0;
CREATE INDEX background_jobs_link_scope_status_idx
    ON background_jobs (
        (payload ->> 'space_id'),
        job_kind,
        (COALESCE(payload ->> 'changed_node_id', '')),
        updated_at DESC,
        job_id DESC
    )
    INCLUDE (status, failure_count)
    WHERE job_kind IN ('node_link_impact', 'node_link_space');

CREATE FUNCTION enqueue_coalesced_node_link_job(TEXT, JSONB)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    queued_job_id UUID;
BEGIN
    INSERT INTO background_jobs (job_kind, payload, available_at, max_attempts)
    VALUES ($1, $2, now(), 8)
    ON CONFLICT DO NOTHING
    RETURNING job_id INTO queued_job_id;

    IF queued_job_id IS NOT NULL THEN
        PERFORM pg_notify('notegate_background_jobs', $1);
    END IF;
END;
$$;

CREATE FUNCTION enqueue_node_link_source(UUID, UUID)
RETURNS BOOLEAN
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL
    ) THEN
        RETURN false;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM nodes
        WHERE space_id = $1 AND id = $2 AND kind = 'text' AND deleted_at IS NULL
    ) AND NOT EXISTS (
        SELECT 1 FROM node_link_source_states
        WHERE space_id = $1 AND source_node_id = $2
    ) AND NOT EXISTS (
        SELECT 1 FROM node_link_refs
        WHERE space_id = $1 AND source_node_id = $2
    ) THEN
        RETURN false;
    END IF;

    PERFORM enqueue_coalesced_node_link_job(
        'node_link_source',
        jsonb_build_object('space_id', $1, 'source_node_id', $2)
    );
    RETURN true;
END;
$$;

CREATE FUNCTION enqueue_node_link_impact(UUID, UUID)
RETURNS BOOLEAN
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL
    ) THEN
        RETURN false;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM nodes WHERE space_id = $1 AND id = $2
    ) THEN
        RETURN false;
    END IF;

    PERFORM enqueue_coalesced_node_link_job(
        'node_link_impact',
        jsonb_build_object('space_id', $1, 'changed_node_id', $2)
    );
    RETURN true;
END;
$$;

CREATE FUNCTION enqueue_node_link_space(UUID)
RETURNS BOOLEAN
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM spaces WHERE id = $1 AND deleted_at IS NULL
    ) THEN
        RETURN false;
    END IF;

    PERFORM enqueue_coalesced_node_link_job(
        'node_link_space',
        jsonb_build_object('space_id', $1)
    );
    RETURN true;
END;
$$;

CREATE FUNCTION enqueue_node_link_index_for_file_change()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.op_type IN ('text.write', 'text.append', 'text.patch', 'text.edit') THEN
        IF NEW.node_id IS NOT NULL THEN
            PERFORM enqueue_node_link_source(NEW.space_id, NEW.node_id);
        END IF;
    ELSIF NEW.op_type IN ('metadata.replace', 'metadata.patch') THEN
        NULL;
    ELSIF NEW.op_type = 'item.update'
          AND COALESCE(NEW.metadata ->> 'name_changed', 'false') <> 'true' THEN
        NULL;
    ELSIF NEW.node_id IS NOT NULL THEN
        PERFORM enqueue_node_link_impact(NEW.space_id, NEW.node_id);
    ELSE
        PERFORM enqueue_node_link_space(NEW.space_id);
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enqueue_node_link_index_for_new_space()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.deleted_at IS NULL THEN
        PERFORM enqueue_node_link_space(NEW.id);
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_enqueue_node_link_index
AFTER INSERT ON spaces
FOR EACH ROW
EXECUTE FUNCTION enqueue_node_link_index_for_new_space();

CREATE TRIGGER file_changes_enqueue_node_link_index
AFTER INSERT ON file_change_events
FOR EACH ROW
EXECUTE FUNCTION enqueue_node_link_index_for_file_change();

SELECT enqueue_node_link_space(id)
FROM spaces
WHERE deleted_at IS NULL;
