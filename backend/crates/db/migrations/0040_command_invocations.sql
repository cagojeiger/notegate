ALTER TABLE mcp_invocations
  RENAME TO command_invocations;

ALTER SEQUENCE mcp_invocations_id_seq
  RENAME TO command_invocations_id_seq;

ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_pkey TO command_invocations_pkey;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_caller_kind_check TO command_invocations_caller_kind_check;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_outcome_check TO command_invocations_outcome_check;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_duration_ms_check TO command_invocations_duration_ms_check;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_purpose_valid TO command_invocations_purpose_valid;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_error_consistent TO command_invocations_error_consistent;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_space_name_valid TO command_invocations_space_name_valid;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_space_name_scope TO command_invocations_space_name_scope;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_input_object TO command_invocations_input_object;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_tool_valid TO command_invocations_tool_valid;
ALTER TABLE command_invocations
  RENAME CONSTRAINT mcp_invocations_response_object TO command_invocations_response_object;

ALTER INDEX mcp_invocations_owner_time_idx
  RENAME TO command_invocations_owner_time_idx;
ALTER INDEX mcp_invocations_actor_time_idx
  RENAME TO command_invocations_actor_time_idx;
ALTER INDEX mcp_invocations_retention_idx
  RENAME TO command_invocations_retention_idx;

ALTER TABLE command_invocations
  ADD COLUMN surface TEXT NOT NULL DEFAULT 'mcp';

ALTER TABLE command_invocations
  ADD CONSTRAINT command_invocations_surface_valid CHECK (
    surface IN ('mcp', 'cli')
  );

DROP INDEX command_invocations_owner_time_idx;
CREATE INDEX command_invocations_owner_surface_time_idx
  ON command_invocations (owner_user_id, surface, created_at DESC, id DESC);

-- Keep the previous relation writable while old and new API replicas overlap
-- during a rolling deployment. PostgreSQL forwards writes through this simple
-- view, and the surface default classifies legacy inserts as MCP calls.
CREATE VIEW mcp_invocations AS
SELECT
  id,
  created_at,
  owner_user_id,
  actor_account_id,
  caller_kind,
  tool,
  op,
  purpose,
  outcome,
  error_code,
  duration_ms,
  space_name,
  input,
  response
FROM command_invocations;
