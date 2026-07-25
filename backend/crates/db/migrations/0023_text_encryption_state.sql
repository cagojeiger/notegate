-- Align the encryption policy with the stored representation before enforcing
-- the invariant.

UPDATE text_objects
SET encryption_enabled = (at_rest_encryption = 'server')
WHERE encryption_enabled IS DISTINCT FROM (at_rest_encryption = 'server');

ALTER TABLE text_objects
    ADD CONSTRAINT text_objects_encryption_state_check
    CHECK (encryption_enabled = (at_rest_encryption = 'server'));
