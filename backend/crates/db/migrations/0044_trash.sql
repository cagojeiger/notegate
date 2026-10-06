-- Existing deletions may already have lost their object bytes. Do not advertise
-- them as recoverable or extend their original purge deadline.
ALTER TABLE nodes ADD COLUMN deletion_root_id UUID;
ALTER TABLE spaces ADD COLUMN trash_recoverable BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX nodes_deletion_root_idx ON nodes(space_id, deletion_root_id)
    WHERE deleted_at IS NOT NULL;
CREATE INDEX spaces_owner_trash_idx ON spaces(owner_user_id, deleted_at DESC, id DESC)
    WHERE deleted_at IS NOT NULL;
CREATE INDEX nodes_trash_roots_idx ON nodes(space_id, deleted_at DESC, id DESC)
    WHERE deleted_at IS NOT NULL AND (deletion_root_id = id OR deletion_root_id IS NULL);
