-- Space-wide id order uses 0019's (space_id, id) index in reverse.
-- Add the equivalent node-scoped access path for history mode.
CREATE INDEX file_change_events_node_id_order_idx
    ON file_change_events (space_id, node_id, id DESC)
    WHERE node_id IS NOT NULL;
