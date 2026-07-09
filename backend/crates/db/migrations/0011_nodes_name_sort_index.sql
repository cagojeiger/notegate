-- Speed up the space-wide name-sorted list, which orders live non-root nodes by name.

CREATE INDEX nodes_live_name_idx
ON nodes (space_id, name, id)
WHERE deleted_at IS NULL
  AND parent_id IS NOT NULL;
