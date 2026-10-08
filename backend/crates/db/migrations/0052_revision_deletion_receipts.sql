-- Observe actual DELETEs, including ordinary maintenance SQL and FK cascades.
-- Do not infer the reason from age: only the deleting path knows its policy.
CREATE FUNCTION record_text_revision_deletion() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    deletion_reason TEXT;
    revision_owner UUID;
BEGIN
    deletion_reason := CASE current_setting('notegate.revision_deletion_reason', true)
        WHEN 'retention' THEN CASE WHEN OLD.checkpoint
            THEN 'checkpoint_expired' ELSE 'intermediate_expired' END
        WHEN 'resource_purge' THEN 'resource_purge'
        ELSE 'unknown'
    END;
    SELECT owner_user_id INTO revision_owner FROM space_storage_usage WHERE space_id = OLD.space_id;
    INSERT INTO audit_events (
        created_at, owner_user_id, source, op_type, resource_type, resource_id, metadata
    ) VALUES (
        clock_timestamp(), revision_owner, 'system', 'text_revision.delete', 'text_revision', OLD.id,
        jsonb_build_object(
            'space_id', OLD.space_id, 'node_id', OLD.node_id,
            'reason', deletion_reason, 'released_bytes', OLD.stored_bytes,
            'completion_scope', 'database', 'recorded_by', 'database_trigger'
        )
    );
    RETURN OLD;
END;
$$;
CREATE TRIGGER text_revisions_record_deletion AFTER DELETE ON text_revisions
    FOR EACH ROW EXECUTE FUNCTION record_text_revision_deletion();
-- Existing audit_events_resource_time_idx supports bounded revision lookups;
-- existing Audit retention removes receipts after 180 days.
