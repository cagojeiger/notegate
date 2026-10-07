-- Preserve existing deletion groups and applied migration checksums.
ALTER TABLE nodes RENAME COLUMN deletion_root_id TO deletion_target_node_id;
ALTER INDEX nodes_deletion_root_idx RENAME TO nodes_deletion_target_idx;
ALTER INDEX nodes_trash_roots_idx RENAME TO nodes_trash_entries_idx;

COMMENT ON COLUMN nodes.deletion_target_node_id IS
    'Node directly targeted by the soft-delete operation that included this node; not its parent or the file-tree root. No FK: correlation does not own resource lifetime.';
COMMENT ON COLUMN spaces.trash_recoverable IS
    'Whether this Space deletion supports trash restoration; current deadline, purge request, content, and limits are checked separately.';
