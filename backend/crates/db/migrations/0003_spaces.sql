-- notegate schema: user-owned spaces and agent-space connections.

CREATE TABLE spaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id UUID NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    deleted_by_user_id UUID REFERENCES users(id),
    purge_after TIMESTAMPTZ,
    CHECK (name ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$'),
    CHECK (
        (deleted_at IS NULL AND deleted_by_user_id IS NULL AND purge_after IS NULL)
        OR (deleted_at IS NOT NULL AND deleted_by_user_id IS NOT NULL AND purge_after IS NOT NULL)
    )
);
CREATE UNIQUE INDEX spaces_owner_name_live_uidx
    ON spaces(owner_user_id, name)
    WHERE deleted_at IS NULL;
CREATE INDEX spaces_owner_list_idx
    ON spaces(owner_user_id, sort_order, name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE space_agent_connections (
    space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN ('read', 'write')),
    connected_by_user_id UUID NOT NULL REFERENCES users(id),
    connected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    disconnected_at TIMESTAMPTZ,
    disconnected_by_user_id UUID REFERENCES users(id),
    PRIMARY KEY (space_id, agent_id),
    CHECK (
        (disconnected_at IS NULL AND disconnected_by_user_id IS NULL)
        OR (disconnected_at IS NOT NULL AND disconnected_by_user_id IS NOT NULL)
    )
);
CREATE INDEX space_agent_connections_agent_live_idx
    ON space_agent_connections(agent_id, space_id)
    WHERE disconnected_at IS NULL;
