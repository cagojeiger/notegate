ALTER TABLE file_change_events
    ADD COLUMN link_index_generation BIGINT
    CHECK (link_index_generation IS NULL OR link_index_generation > 0);

CREATE TABLE space_link_index_states (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    desired_generation BIGINT NOT NULL DEFAULT 0 CHECK (desired_generation >= 0),
    applied_generation BIGINT NOT NULL DEFAULT 0 CHECK (applied_generation >= 0),
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'rebuilding', 'ready', 'failed')),
    rebuild_requested BOOLEAN NOT NULL DEFAULT true,
    rebuild_base_generation BIGINT CHECK (rebuild_base_generation IS NULL OR rebuild_base_generation >= 0),
    rebuild_after_node_id UUID,
    parser_version INTEGER NOT NULL DEFAULT 1 CHECK (parser_version > 0),
    claim_token UUID,
    claim_until TIMESTAMPTZ,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    last_indexed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (applied_generation <= desired_generation),
    CHECK ((claim_token IS NULL) = (claim_until IS NULL))
);

CREATE INDEX space_link_index_states_ready_idx
    ON space_link_index_states (run_after, updated_at, space_id)
    WHERE status <> 'ready' OR applied_generation < desired_generation OR rebuild_requested;

CREATE UNIQUE INDEX file_change_events_link_index_generation_idx
    ON file_change_events (space_id, link_index_generation)
    WHERE link_index_generation IS NOT NULL;

CREATE TABLE node_link_refs (
    id BIGSERIAL PRIMARY KEY,
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL,
    target_node_id UUID,
    reference_kind TEXT NOT NULL CHECK (reference_kind IN ('link', 'image')),
    raw_href TEXT NOT NULL CHECK (raw_href <> ''),
    normalized_target_path TEXT,
    occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id) ON DELETE CASCADE,
    FOREIGN KEY (target_node_id, space_id)
        REFERENCES nodes(id, space_id) ON DELETE SET NULL (target_node_id)
);

CREATE INDEX node_link_refs_outgoing_idx
    ON node_link_refs (space_id, source_node_id, id);

CREATE INDEX node_link_refs_incoming_idx
    ON node_link_refs (space_id, target_node_id, id)
    WHERE target_node_id IS NOT NULL;

CREATE INDEX node_link_refs_target_path_hash_idx
    ON node_link_refs (space_id, md5(normalized_target_path))
    WHERE normalized_target_path IS NOT NULL;

CREATE INDEX nodes_live_text_link_rebuild_idx
    ON nodes (space_id, id)
    WHERE kind = 'text'
      AND parent_id IS NOT NULL
      AND deleted_at IS NULL;

CREATE OR REPLACE FUNCTION create_space_link_index_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO space_link_index_states (space_id)
    VALUES (NEW.id)
    ON CONFLICT (space_id) DO NOTHING;
    RETURN NEW;
END;
$$;

CREATE TRIGGER spaces_create_link_index_state
AFTER INSERT ON spaces
FOR EACH ROW
EXECUTE FUNCTION create_space_link_index_state();

CREATE OR REPLACE FUNCTION queue_space_link_index()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    next_generation BIGINT;
BEGIN
    INSERT INTO space_link_index_states (space_id)
    VALUES (NEW.space_id)
    ON CONFLICT (space_id) DO NOTHING;

    UPDATE space_link_index_states
    SET desired_generation = desired_generation + 1,
        status = CASE
            WHEN status IN ('running', 'rebuilding') THEN status
            ELSE 'queued'
        END,
        run_after = LEAST(run_after, now()),
        updated_at = now()
    WHERE space_id = NEW.space_id
    RETURNING desired_generation INTO next_generation;

    NEW.link_index_generation := next_generation;
    RETURN NEW;
END;
$$;

CREATE TRIGGER file_change_events_queue_link_index
BEFORE INSERT ON file_change_events
FOR EACH ROW
EXECUTE FUNCTION queue_space_link_index();

INSERT INTO space_link_index_states (
    space_id,
    desired_generation,
    applied_generation,
    status,
    rebuild_requested
)
SELECT
    s.id,
    0,
    0,
    'queued',
    true
FROM spaces s
ON CONFLICT (space_id) DO NOTHING;
