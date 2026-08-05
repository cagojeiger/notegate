CREATE TABLE node_link_refs (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL,
    target_node_id UUID REFERENCES nodes(id) ON DELETE SET NULL,
    target_path TEXT NOT NULL,
    reference_kind TEXT NOT NULL CHECK (reference_kind IN ('link', 'image')),
    occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
    PRIMARY KEY (space_id, source_node_id, reference_kind, target_path),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (target_path LIKE '/%' AND octet_length(target_path) <= 4096)
);

CREATE INDEX node_link_refs_incoming_idx
    ON node_link_refs (space_id, target_node_id, source_node_id)
    WHERE target_node_id IS NOT NULL;

CREATE TABLE node_link_source_states (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL,
    requested_version BIGINT NOT NULL DEFAULT 1 CHECK (requested_version > 0),
    applied_version BIGINT NOT NULL DEFAULT 0 CHECK (applied_version >= 0),
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    claim_token UUID,
    claim_until TIMESTAMPTZ,
    last_error TEXT,
    last_synced_at TIMESTAMPTZ,
    PRIMARY KEY (space_id, source_node_id),
    FOREIGN KEY (source_node_id, space_id)
        REFERENCES nodes(id, space_id)
        ON DELETE CASCADE,
    CHECK (applied_version <= requested_version),
    CHECK ((claim_token IS NULL) = (claim_until IS NULL))
);

CREATE INDEX node_link_source_states_ready_idx
    ON node_link_source_states (run_after, source_node_id)
    WHERE requested_version > applied_version;

CREATE TABLE node_link_space_reindex_states (
    space_id UUID PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    requested_version BIGINT NOT NULL DEFAULT 1 CHECK (requested_version > 0),
    applied_version BIGINT NOT NULL DEFAULT 0 CHECK (applied_version >= 0),
    run_after TIMESTAMPTZ NOT NULL DEFAULT now(),
    claim_token UUID,
    claim_until TIMESTAMPTZ,
    last_error TEXT,
    CHECK (applied_version <= requested_version),
    CHECK ((claim_token IS NULL) = (claim_until IS NULL))
);

CREATE INDEX node_link_space_reindex_states_ready_idx
    ON node_link_space_reindex_states (run_after, space_id)
    WHERE requested_version > applied_version;

INSERT INTO node_link_space_reindex_states (space_id)
SELECT id FROM spaces WHERE deleted_at IS NULL;
