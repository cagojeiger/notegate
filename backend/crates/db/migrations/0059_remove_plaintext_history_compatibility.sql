-- Finish history encryption with v0.1.112 before stopping all writers to upgrade.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM file_change_events WHERE private_metadata IS NULL)
       OR EXISTS (SELECT 1 FROM command_invocations WHERE private_payload IS NULL)
       OR EXISTS (SELECT 1 FROM text_objects WHERE revision_purpose IS NOT NULL)
       OR EXISTS (SELECT 1 FROM text_revisions WHERE purpose IS NOT NULL) THEN
        RAISE EXCEPTION 'Complete history encryption with v0.1.112 before migration 59';
    END IF;
END $$;

DROP VIEW mcp_invocations;
DROP INDEX file_change_events_unencrypted_idx;
DROP INDEX command_invocations_unencrypted_idx;
ALTER TABLE file_change_events
    ALTER COLUMN snapshot_id SET NOT NULL,
    ALTER COLUMN private_metadata SET NOT NULL;
ALTER TABLE command_invocations
    DROP COLUMN purpose,
    DROP COLUMN space_name,
    DROP COLUMN input,
    DROP COLUMN response,
    ALTER COLUMN snapshot_id SET NOT NULL,
    ALTER COLUMN private_payload SET NOT NULL;

DROP TRIGGER text_revisions_preserve_private_purpose ON text_revisions;
DROP FUNCTION preserve_revision_private_purpose();
DROP TRIGGER text_objects_clear_replaced_private_purpose ON text_objects;
DROP FUNCTION clear_replaced_private_purpose();
ALTER TABLE text_objects DROP COLUMN revision_purpose;
ALTER TABLE text_revisions DROP COLUMN purpose;
