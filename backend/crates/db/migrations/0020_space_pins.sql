-- User-owned Space pins define the Space set exposed to user-authenticated MCP.
-- Existing Spaces remain exposed after rollout; newly created Spaces start unpinned.

ALTER TABLE spaces
    ADD COLUMN pinned_at TIMESTAMPTZ;

UPDATE spaces
SET pinned_at = now()
WHERE deleted_at IS NULL;

CREATE INDEX spaces_owner_pinned_list_idx
    ON spaces(owner_user_id, sort_order, name, id)
    WHERE deleted_at IS NULL AND pinned_at IS NOT NULL;
