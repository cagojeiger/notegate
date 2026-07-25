DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM text_objects
        WHERE encryption_enabled IS DISTINCT FROM (at_rest_encryption = 'server')
    ) THEN
        RAISE EXCEPTION
            'text encryption policy differs from stored state; materialize the policy before migration';
    END IF;
END
$$;

ALTER TABLE text_objects
    ADD CONSTRAINT text_objects_encryption_state_check
    CHECK (encryption_enabled = (at_rest_encryption = 'server'));
