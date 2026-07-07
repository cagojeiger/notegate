CREATE TABLE audit_events (
  id bigserial PRIMARY KEY,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  owner_user_id uuid NULL,
  actor_account_id uuid NULL,
  source text NOT NULL CHECK (source IN ('rest', 'mcp', 'system')),
  op_type text NOT NULL,
  resource_type text NOT NULL,
  resource_id uuid NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  CONSTRAINT audit_events_metadata_object CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE INDEX audit_events_owner_time_idx
  ON audit_events (owner_user_id, occurred_at DESC, id DESC);

CREATE INDEX audit_events_actor_time_idx
  ON audit_events (actor_account_id, occurred_at DESC, id DESC);

CREATE INDEX audit_events_resource_time_idx
  ON audit_events (resource_type, resource_id, occurred_at DESC, id DESC);

CREATE INDEX audit_events_retention_idx
  ON audit_events (occurred_at);
