-- Keep the compatibility columns readable during rolling deployments. New
-- writers leave them empty and put document-related fields in an AEAD envelope.
ALTER TABLE command_invocations
  ADD COLUMN invocation_id UUID,
  ADD COLUMN snapshot_id UUID,
  ADD COLUMN private_payload JSONB,
  ADD CONSTRAINT command_invocations_encrypted_payload CHECK (
    (snapshot_id IS NULL) = (private_payload IS NULL)
    AND (private_payload IS NULL OR (
      purpose IS NULL AND space_name IS NULL AND input = '{}'::jsonb AND response IS NULL
    ))
  );

CREATE INDEX command_invocations_unencrypted_idx ON command_invocations(id)
  WHERE private_payload IS NULL;

CREATE UNIQUE INDEX command_invocations_invocation_id_idx ON command_invocations(invocation_id)
  WHERE invocation_id IS NOT NULL;
