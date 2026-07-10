CREATE TABLE audit_events (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  owner_user_id UUID NULL,
  actor_account_id UUID NULL,
  source TEXT NOT NULL CHECK (source IN ('rest', 'mcp', 'system')),
  op_type TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  resource_id UUID NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  CONSTRAINT audit_events_metadata_object CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE INDEX audit_events_owner_time_idx
  ON audit_events (owner_user_id, created_at DESC, id DESC);

CREATE INDEX audit_events_actor_time_idx
  ON audit_events (actor_account_id, created_at DESC, id DESC);

CREATE INDEX audit_events_resource_time_idx
  ON audit_events (resource_type, resource_id, created_at DESC, id DESC);

CREATE INDEX audit_events_retention_idx
  ON audit_events (created_at);
