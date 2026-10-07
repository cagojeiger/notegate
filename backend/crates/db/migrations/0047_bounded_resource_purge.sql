-- Scheduling metadata only: an attempt is not a deletion/completion receipt.
ALTER TABLE spaces ADD COLUMN purge_last_attempt_at TIMESTAMPTZ;
CREATE INDEX spaces_purge_rotation_idx ON spaces(purge_last_attempt_at NULLS FIRST, id);
CREATE INDEX nodes_space_purge_idx ON nodes(space_id, purge_after, id)
    WHERE deleted_at IS NOT NULL;
-- Physical children include independently trashed nodes; the live-tree index
-- cannot establish that a folder is empty before hard deletion.
CREATE INDEX nodes_physical_children_idx ON nodes(space_id, parent_id, id);
CREATE INDEX object_storage_objects_parent_idx ON object_storage_objects(parent_node_id, id)
    WHERE parent_node_id IS NOT NULL;
CREATE INDEX object_storage_objects_space_idx ON object_storage_objects(space_id, id)
    WHERE space_id IS NOT NULL;
