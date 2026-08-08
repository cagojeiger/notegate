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
    CHECK (target_path LIKE '/%' AND octet_length(target_path) <= 4096)
);

CREATE INDEX node_link_refs_incoming_idx
    ON node_link_refs (space_id, target_node_id, source_node_id, reference_kind)
    WHERE target_node_id IS NOT NULL;

CREATE TABLE reconciliation_work_items (
    queue_name TEXT NOT NULL,
    work_kind TEXT NOT NULL,
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    target_id UUID NOT NULL,
    requested_generation BIGINT NOT NULL DEFAULT 1 CHECK (requested_generation > 0),
    applied_generation BIGINT NOT NULL DEFAULT 0 CHECK (applied_generation >= 0),
    claimed_generation BIGINT,
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    claim_token UUID,
    lease_until TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error TEXT,
    last_completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (queue_name, work_kind, target_id),
    CHECK (octet_length(queue_name) BETWEEN 1 AND 128),
    CHECK (octet_length(work_kind) BETWEEN 1 AND 128),
    CHECK (applied_generation <= requested_generation),
    CHECK (
        claimed_generation IS NULL OR (
            claimed_generation > applied_generation
            AND claimed_generation <= requested_generation
        )
    ),
    CHECK (
        (claimed_generation IS NULL AND claim_token IS NULL AND lease_until IS NULL)
        OR
        (claimed_generation IS NOT NULL AND claim_token IS NOT NULL AND lease_until IS NOT NULL)
    )
);

CREATE INDEX reconciliation_work_items_ready_idx
    ON reconciliation_work_items (queue_name, run_after, created_at, target_id)
    WHERE requested_generation > applied_generation;

CREATE FUNCTION reconciliation_backlog(TEXT)
RETURNS BIGINT
LANGUAGE SQL
STABLE
AS $$
    SELECT count(*)::BIGINT
    FROM reconciliation_work_items
    WHERE queue_name = $1
      AND requested_generation > applied_generation
      AND (run_after <= now() OR lease_until > now());
$$;

CREATE FUNCTION enqueue_reconciliation_work(TEXT, TEXT, UUID, UUID)
RETURNS BOOLEAN
LANGUAGE SQL
AS $$
    WITH requested AS (
        INSERT INTO reconciliation_work_items (
            queue_name, work_kind, space_id, target_id
        ) VALUES ($1, $2, $3, $4)
        ON CONFLICT (queue_name, work_kind, target_id) DO UPDATE
        SET space_id = EXCLUDED.space_id,
            requested_generation = reconciliation_work_items.requested_generation + 1,
            run_after = now(),
            attempt_count = 0,
            last_error = NULL,
            updated_at = now()
        RETURNING queue_name
    ), notified AS (
        SELECT pg_notify('notegate_reconciliation', queue_name)
        FROM requested
    )
    SELECT EXISTS (SELECT 1 FROM notified);
$$;

CREATE FUNCTION enqueue_node_link_source(UUID, UUID)
RETURNS BOOLEAN
LANGUAGE SQL
AS $$
    WITH requested AS (
        SELECT enqueue_reconciliation_work(
            'projection', 'node_link_source', node.space_id, node.id
        )
        FROM nodes node
        WHERE node.space_id = $1
          AND node.id = $2
          AND node.kind = 'text'
          AND node.deleted_at IS NULL
    )
    SELECT EXISTS (SELECT 1 FROM requested);
$$;

CREATE FUNCTION enqueue_node_link_space(UUID)
RETURNS BOOLEAN
LANGUAGE SQL
AS $$
    WITH requested AS (
        SELECT enqueue_reconciliation_work(
            'projection', 'node_link_space', id, id
        )
        FROM spaces
        WHERE id = $1 AND deleted_at IS NULL
    )
    SELECT EXISTS (SELECT 1 FROM requested);
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
    ELSE
        PERFORM enqueue_node_link_space(NEW.space_id);
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER file_changes_enqueue_node_link_index
AFTER INSERT ON file_change_events
FOR EACH ROW
EXECUTE FUNCTION enqueue_node_link_index_for_file_change();

SELECT enqueue_node_link_space(id)
FROM spaces
WHERE deleted_at IS NULL;
