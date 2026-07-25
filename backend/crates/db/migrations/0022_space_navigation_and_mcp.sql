-- Separate workspace navigation preference from user MCP authorization.
-- Preserve the previous combined state for existing Spaces.

ALTER TABLE spaces
    RENAME COLUMN pinned_at TO user_mcp_enabled_at;

ALTER TABLE spaces
    ADD COLUMN navigation_pinned_at TIMESTAMPTZ;

UPDATE spaces
SET navigation_pinned_at = user_mcp_enabled_at
WHERE user_mcp_enabled_at IS NOT NULL;

ALTER INDEX spaces_owner_pinned_list_idx
    RENAME TO spaces_owner_mcp_enabled_list_idx;
