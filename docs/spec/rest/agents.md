# REST Agents

## Agents

Agent APIs manage agent accounts and API keys. Workspace-specific permissions for agents are still granted through the Access category. API keys authenticate as `agent` accounts. Agent key lifecycle is governed by agent ownership/creator rules, not by workspace role; workspace owners only grant or revoke workspace access for agent accounts.

### List agents

```http
GET /api/v1/agents?limit=100&cursor=...
```

Returns agents created by or visible to the caller. Default and max limit are `100`.

### Create agent

```http
POST /api/v1/agents
```

```json
{
  "name": "research-agent"
}
```

Creates an `agent` account. Access to workspaces is granted separately through workspace access APIs. A creator account has at most `50` active agents.

### Delete agent

```http
DELETE /api/v1/agents/{agent_id}
```

Soft-deactivates the underlying account, revokes active keys, and revokes workspace access.

### Create agent key

```http
POST /api/v1/agents/{agent_id}/keys
```

```json
{
  "name": "local-mcp",
  "expires_at": "2026-12-31T00:00:00Z",
  "scopes": []
}
```

Returns the plaintext key exactly once.

Branching:

```text
active keys < 10     -> create key
active keys >= 10    -> 409 conflict
scopes omitted or [] -> allowed
scopes non-empty     -> 400 invalid input
```

### Revoke agent key

```http
DELETE /api/v1/agents/{agent_id}/keys/{key_id}
```

Sets `revoked_at`/`revoked_by`.
