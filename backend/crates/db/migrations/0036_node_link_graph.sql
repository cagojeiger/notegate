CREATE TABLE node_link_refs (
    space_id UUID NOT NULL,
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

CREATE INDEX nodes_live_text_link_scan_idx
    ON nodes (space_id, id)
    WHERE kind = 'text' AND deleted_at IS NULL;

CREATE TABLE node_link_projections (
    space_id UUID NOT NULL,
    source_node_id UUID NOT NULL,
    projected_at TIMESTAMPTZ,
    needs_projection BOOLEAN NOT NULL DEFAULT false,
    request_version BIGINT NOT NULL DEFAULT 1 CHECK (request_version > 0),
    active_job_id UUID REFERENCES background_jobs(job_id) ON DELETE SET NULL,
    active_request_version BIGINT,
    failure_code TEXT,
    failed_at TIMESTAMPTZ,
    PRIMARY KEY (space_id, source_node_id),
    CHECK (active_request_version IS NULL OR active_request_version <= request_version),
    CHECK ((failure_code IS NULL) = (failed_at IS NULL)),
    CHECK (NOT needs_projection OR failure_code IS NULL),
    CHECK (failure_code IS NULL OR octet_length(failure_code) BETWEEN 1 AND 128)
);

CREATE INDEX node_link_projections_ready_idx
    ON node_link_projections (space_id, source_node_id)
    WHERE needs_projection AND active_job_id IS NULL AND failed_at IS NULL;

CREATE INDEX node_link_projections_job_idx
    ON node_link_projections (active_job_id)
    WHERE active_job_id IS NOT NULL;

CREATE TABLE link_graph_space_states (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    last_processed_event_id BIGINT NOT NULL DEFAULT 0
        CHECK (last_processed_event_id >= 0),
    available_at TIMESTAMPTZ,
    pending_since_event_id BIGINT CHECK (pending_since_event_id > 0),
    incremental_event_id BIGINT CHECK (incremental_event_id >= 0),
    full_scan_event_id BIGINT CHECK (full_scan_event_id >= 0),
    full_scan_after_node_id UUID,
    CHECK (incremental_event_id IS NULL OR incremental_event_id >= last_processed_event_id),
    CHECK (full_scan_event_id IS NULL OR full_scan_event_id >= last_processed_event_id),
    CHECK (NOT (incremental_event_id IS NOT NULL AND full_scan_event_id IS NOT NULL)),
    CHECK (full_scan_event_id IS NOT NULL OR full_scan_after_node_id IS NULL),
    CHECK (
        available_at IS NOT NULL
        OR (
            pending_since_event_id IS NULL
            AND incremental_event_id IS NULL
            AND full_scan_event_id IS NULL
        )
    )
);

CREATE INDEX link_graph_space_states_due_idx
    ON link_graph_space_states (
        ((incremental_event_id IS NOT NULL OR full_scan_event_id IS NOT NULL)) DESC,
        available_at,
        space_id
    )
    WHERE available_at IS NOT NULL;

INSERT INTO link_graph_space_states (
    space_id,
    available_at,
    full_scan_event_id
)
SELECT space.id, now(), COALESCE(max(event.id), 0)
FROM spaces space
LEFT JOIN file_change_events event ON event.space_id = space.id
WHERE space.deleted_at IS NULL
GROUP BY space.id;

CREATE OR REPLACE FUNCTION mark_link_graph_space_pending()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO link_graph_space_states (
        space_id,
        available_at,
        pending_since_event_id
    ) VALUES (
        NEW.space_id,
        clock_timestamp() + interval '5 minutes',
        NEW.id
    )
    ON CONFLICT (space_id) DO UPDATE
    SET available_at = GREATEST(
            COALESCE(link_graph_space_states.available_at, '-infinity'::timestamptz),
            clock_timestamp() + interval '5 minutes'
        ),
        pending_since_event_id = COALESCE(
            link_graph_space_states.pending_since_event_id,
            EXCLUDED.pending_since_event_id
        );

    RETURN NEW;
END;
$$;

CREATE TRIGGER file_change_events_mark_link_graph_pending
AFTER INSERT ON file_change_events
FOR EACH ROW
EXECUTE FUNCTION mark_link_graph_space_pending();

CREATE OR REPLACE FUNCTION mark_deleted_space_link_graph_pending()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL THEN
        INSERT INTO link_graph_space_states (space_id, available_at)
        VALUES (NEW.id, now())
        ON CONFLICT (space_id) DO UPDATE
        SET available_at = now();
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_mark_link_graph_pending_on_delete
AFTER UPDATE OF deleted_at ON spaces
FOR EACH ROW
EXECUTE FUNCTION mark_deleted_space_link_graph_pending();
