ALTER TABLE node_link_source_states
    RENAME COLUMN last_synced_at TO projected_at;

ALTER TABLE node_link_space_states
    RENAME COLUMN last_synced_at TO expanded_at;
