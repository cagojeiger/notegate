CREATE TABLE file_change_events (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  space_id UUID NOT NULL,
  node_id UUID NULL,
  actor_account_id UUID NULL,
  op_type TEXT NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  CONSTRAINT file_change_events_metadata_object CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE INDEX file_change_events_space_time_idx
  ON file_change_events (space_id, created_at DESC, id DESC);

CREATE INDEX file_change_events_node_time_idx
  ON file_change_events (space_id, node_id, created_at DESC, id DESC);

CREATE INDEX file_change_events_actor_time_idx
  ON file_change_events (actor_account_id, created_at DESC, id DESC);

CREATE INDEX file_change_events_retention_idx
  ON file_change_events (created_at);
