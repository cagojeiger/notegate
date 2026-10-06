-- Correlation identifiers are independent of resource/event lifetimes. Older
-- rows remain NULL: do not infer an operation from a node ID or timestamp.
ALTER TABLE nodes ADD COLUMN deletion_operation_id UUID;
ALTER TABLE spaces ADD COLUMN deletion_operation_id UUID;
ALTER TABLE file_change_events ADD COLUMN operation_id UUID;
ALTER TABLE audit_events ADD COLUMN operation_id UUID;
ALTER TABLE object_storage_objects ADD COLUMN deletion_operation_id UUID;

ALTER TABLE nodes ADD CHECK (deletion_operation_id IS NULL OR deleted_at IS NOT NULL);
ALTER TABLE spaces ADD CHECK (deletion_operation_id IS NULL OR deleted_at IS NOT NULL);

CREATE INDEX file_change_events_operation_idx ON file_change_events(space_id, operation_id)
    WHERE operation_id IS NOT NULL;
CREATE INDEX audit_events_operation_idx ON audit_events(owner_user_id, operation_id)
    WHERE operation_id IS NOT NULL;
