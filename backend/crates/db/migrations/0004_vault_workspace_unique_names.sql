DROP INDEX IF EXISTS nodes_live_sibling_name_key;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_live_workspace_name_key
    ON nodes(workspace_id, name)
    WHERE deleted_at IS NULL AND parent_id IS NOT NULL;
