-- Snapshot ownership survives resource removal. Legacy ownership is recovered
-- only from an existing Space, never guessed from actor or timestamps.
ALTER TABLE file_change_events ADD COLUMN owner_user_id UUID;
ALTER TABLE file_change_events ADD COLUMN private_metadata JSONB;
ALTER TABLE file_change_events ADD COLUMN snapshot_id UUID;
ALTER TABLE file_change_events ADD CHECK ((snapshot_id IS NULL) = (private_metadata IS NULL));
CREATE FUNCTION initialize_change_history_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.owner_user_id IS NULL THEN
        SELECT owner_user_id INTO NEW.owner_user_id FROM spaces WHERE id = NEW.space_id;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER initialize_change_history_owner BEFORE INSERT ON file_change_events
    FOR EACH ROW EXECUTE FUNCTION initialize_change_history_owner();
CREATE INDEX file_change_events_owner_time_idx
    ON file_change_events(owner_user_id, created_at DESC, id DESC)
    WHERE owner_user_id IS NOT NULL;
-- Existing plaintext payloads are encrypted in bounded application batches.
CREATE INDEX file_change_events_unencrypted_idx ON file_change_events(id)
    WHERE private_metadata IS NULL;
-- Bounded metadata-only lookups for current version references.
CREATE UNIQUE INDEX text_objects_revision_id_idx ON text_objects(revision_id);
