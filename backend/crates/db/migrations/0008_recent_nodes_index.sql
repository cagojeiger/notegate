-- Speed up the space-wide Recent list, which orders live non-root nodes by update time.

CREATE INDEX nodes_live_recent_idx
ON nodes (space_id, updated_at DESC, id DESC)
WHERE deleted_at IS NULL
  AND parent_id IS NOT NULL;
