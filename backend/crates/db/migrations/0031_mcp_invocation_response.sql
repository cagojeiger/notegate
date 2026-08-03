ALTER TABLE mcp_invocations
  ADD COLUMN response JSONB;

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_response_object CHECK (
    response IS NULL OR jsonb_typeof(response) = 'object'
  );
