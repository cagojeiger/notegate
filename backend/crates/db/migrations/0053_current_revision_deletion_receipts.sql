-- The current body also owns a revision ID referenced by Changes.
-- Ordinary saves archive/UPDATE it; only an actual DELETE completes its removal.
CREATE FUNCTION record_current_text_revision_deletion() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    deletion_reason TEXT;
    revision_owner UUID;
BEGIN
    deletion_reason := CASE current_setting('notegate.revision_deletion_reason', true)
        WHEN 'resource_purge' THEN 'resource_purge'
        ELSE 'unknown'
    END;
    SELECT owner_user_id INTO revision_owner FROM space_storage_usage WHERE space_id = OLD.space_id;
    INSERT INTO audit_events (
        created_at, owner_user_id, source, op_type, resource_type, resource_id, metadata
    ) VALUES (
        clock_timestamp(), revision_owner, 'system', 'text_revision.delete', 'text_revision', OLD.revision_id,
        jsonb_build_object(
            'space_id', OLD.space_id, 'node_id', OLD.node_id,
            'reason', deletion_reason, 'released_bytes', OLD.byte_len,
            'completion_scope', 'database', 'recorded_by', 'database_trigger'
        )
    );
    RETURN OLD;
END;
$$;
CREATE TRIGGER text_objects_record_revision_deletion AFTER DELETE ON text_objects
    FOR EACH ROW EXECUTE FUNCTION record_current_text_revision_deletion();
