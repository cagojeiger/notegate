CREATE INDEX node_link_projections_pending_space_idx
    ON node_link_projections (space_id)
    WHERE needs_projection;
