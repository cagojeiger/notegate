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

CREATE TABLE node_link_source_states (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL,
    source_content_sha256 TEXT NOT NULL,
    source_path TEXT NOT NULL,
    parser_version INTEGER NOT NULL CHECK (parser_version > 0),
    projected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (space_id, source_node_id),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (source_path LIKE '/%' AND octet_length(source_path) <= 903)
);

CREATE INDEX nodes_live_text_link_scan_idx
    ON nodes (space_id, id)
    WHERE kind = 'text' AND deleted_at IS NULL;

CREATE SEQUENCE node_link_projection_request_version_seq AS BIGINT;

CREATE TABLE node_link_projection_targets (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    node_id UUID NOT NULL,
    request_version BIGINT NOT NULL
        DEFAULT nextval('node_link_projection_request_version_seq')
        CHECK (request_version > 0),
    active_job_id UUID REFERENCES background_jobs(job_id) ON DELETE SET NULL,
    active_request_version BIGINT,
    failure_code TEXT,
    failed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (space_id, node_id),
    FOREIGN KEY (node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (active_request_version IS NULL OR active_request_version <= request_version),
    CHECK ((failure_code IS NULL) = (failed_at IS NULL)),
    CHECK (failure_code IS NULL OR octet_length(failure_code) BETWEEN 1 AND 128)
);

ALTER SEQUENCE node_link_projection_request_version_seq
    OWNED BY node_link_projection_targets.request_version;

CREATE INDEX node_link_projection_targets_ready_idx
    ON node_link_projection_targets (space_id, node_id)
    WHERE active_job_id IS NULL AND failed_at IS NULL;

CREATE INDEX node_link_projection_targets_job_idx
    ON node_link_projection_targets (active_job_id)
    WHERE active_job_id IS NOT NULL;

CREATE TABLE space_change_processor_states (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    processor_kind TEXT NOT NULL,
    last_processed_event_id BIGINT NOT NULL DEFAULT 0
        CHECK (last_processed_event_id >= 0),
    processing_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (processing_state IN ('idle', 'pending')),
    available_at TIMESTAMPTZ DEFAULT now(),
    requires_full_scan BOOLEAN NOT NULL DEFAULT true,
    full_scan_event_id BIGINT CHECK (full_scan_event_id >= 0),
    full_scan_after_node_id UUID,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (space_id, processor_kind),
    CHECK (octet_length(processor_kind) BETWEEN 1 AND 127),
    CHECK (
        (processing_state = 'idle' AND available_at IS NULL)
        OR (processing_state = 'pending' AND available_at IS NOT NULL)
    ),
    CHECK (NOT requires_full_scan OR processing_state = 'pending'),
    CHECK (
        requires_full_scan
        OR (full_scan_event_id IS NULL AND full_scan_after_node_id IS NULL)
    ),
    CHECK ((full_scan_event_id IS NULL) = (full_scan_after_node_id IS NULL))
);

CREATE INDEX space_change_processor_pending_idx
    ON space_change_processor_states (processor_kind, available_at, space_id)
    WHERE processing_state = 'pending';

INSERT INTO space_change_processor_states (
    space_id,
    processor_kind,
    processing_state,
    available_at,
    requires_full_scan
)
SELECT id, 'link_graph', 'pending', now(), true
FROM spaces
WHERE deleted_at IS NULL;

CREATE OR REPLACE FUNCTION mark_space_change_processors_pending()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO space_change_processor_states (space_id, processor_kind)
    VALUES (NEW.space_id, 'link_graph')
    ON CONFLICT (space_id, processor_kind) DO NOTHING;

    -- The unconditional update serializes a concurrent collector with this
    -- event transaction, so an idle transition cannot lose the wakeup.
    UPDATE space_change_processor_states
    SET processing_state = 'pending',
        available_at = now(),
        updated_at = now()
    WHERE space_id = NEW.space_id;

    RETURN NEW;
END;
$$;

CREATE TRIGGER file_change_events_mark_processors_pending
AFTER INSERT ON file_change_events
FOR EACH ROW
EXECUTE FUNCTION mark_space_change_processors_pending();

CREATE OR REPLACE FUNCTION mark_deleted_space_processors_pending()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL THEN
        INSERT INTO space_change_processor_states (
            space_id,
            processor_kind
        ) VALUES (NEW.id, 'link_graph')
        ON CONFLICT (space_id, processor_kind) DO NOTHING;

        UPDATE space_change_processor_states
        SET processing_state = 'pending',
            available_at = now(),
            updated_at = now()
        WHERE space_id = NEW.id;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_mark_processors_pending_on_delete
AFTER UPDATE OF deleted_at ON spaces
FOR EACH ROW
EXECUTE FUNCTION mark_deleted_space_processors_pending();
