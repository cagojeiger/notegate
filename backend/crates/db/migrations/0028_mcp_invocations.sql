CREATE TABLE mcp_invocations (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  owner_user_id UUID NOT NULL,
  actor_account_id UUID NOT NULL,
  caller_kind TEXT NOT NULL CHECK (caller_kind IN ('user', 'agent')),
  tool TEXT NOT NULL CHECK (
    tool IN ('me', 'read', 'search', 'write', 'manage', 'file_transfer', 'run_sequence')
  ),
  op TEXT,
  purpose TEXT,
  outcome TEXT NOT NULL CHECK (outcome IN ('success', 'error')),
  error_code TEXT,
  duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
  CONSTRAINT mcp_invocations_purpose_valid CHECK (
    (tool = 'me' AND purpose IS NULL)
    OR (
      tool <> 'me'
      AND purpose IS NOT NULL
      AND char_length(purpose) BETWEEN 1 AND 200
      AND purpose !~ '^[[:space:]]|[[:space:]]$'
    )
  ),
  CONSTRAINT mcp_invocations_error_consistent CHECK (
    (outcome = 'success' AND error_code IS NULL)
    OR (outcome = 'error' AND error_code IS NOT NULL)
  )
);

CREATE INDEX mcp_invocations_owner_time_idx
  ON mcp_invocations (owner_user_id, created_at DESC, id DESC);

CREATE INDEX mcp_invocations_actor_time_idx
  ON mcp_invocations (actor_account_id, created_at DESC, id DESC);

CREATE INDEX mcp_invocations_retention_idx
  ON mcp_invocations (created_at);
