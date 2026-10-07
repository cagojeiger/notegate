-- Snapshot ownership survives resource removal. Legacy ownership is recovered
-- only from an existing Space, never guessed from actor or timestamps.
ALTER TABLE file_change_events ADD COLUMN owner_user_id UUID;
ALTER TABLE file_change_events ADD COLUMN private_metadata JSONB;
UPDATE file_change_events e SET owner_user_id = s.owner_user_id
FROM spaces s WHERE s.id = e.space_id;
CREATE INDEX file_change_events_owner_time_idx
    ON file_change_events(owner_user_id, created_at DESC, id DESC)
    WHERE owner_user_id IS NOT NULL;
-- Existing plaintext payloads are encrypted in bounded application batches.
CREATE INDEX file_change_events_unencrypted_idx ON file_change_events(id)
    WHERE private_metadata IS NULL;
-- Bounded metadata-only lookups for current version references.
CREATE UNIQUE INDEX text_objects_revision_id_idx ON text_objects(revision_id);
