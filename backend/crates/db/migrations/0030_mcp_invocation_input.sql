ALTER TABLE mcp_invocations
  ADD COLUMN input JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE mcp_invocations
  ALTER COLUMN input DROP DEFAULT;

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_input_object CHECK (jsonb_typeof(input) = 'object');

ALTER TABLE mcp_invocations
  DROP CONSTRAINT mcp_invocations_tool_check;

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_tool_valid CHECK (
    char_length(tool) BETWEEN 1 AND 128
    AND tool = btrim(tool)
    AND tool !~ '[[:cntrl:]]'
  );

ALTER TABLE mcp_invocations
  DROP CONSTRAINT mcp_invocations_purpose_valid;

ALTER TABLE mcp_invocations
  ADD CONSTRAINT mcp_invocations_purpose_valid CHECK (
    purpose IS NULL
    OR (
      char_length(purpose) BETWEEN 1 AND 200
      AND purpose !~ '^[[:space:]]|[[:space:]]$'
    )
  );
