# REST Agents

Agent는 user가 관리하는 worker account다. Agent caller는 agent management API를 호출할 수 없다.

## List agents

```http
GET /api/v1/agents?limit=100&cursor=...
```

User caller가 소유한 active agent 목록을 반환한다.

## Create agent

```http
POST /api/v1/agents
```

```json
{"name":"research-agent"}
```

Side effect:

```text
accounts(kind='agent')
agents(owner_user_id=caller)
```

API key와 space connection은 자동 생성하지 않는다.

## Delete agent

```http
DELETE /api/v1/agents/{agent_id}
```

Owner user만 가능하다. Agent account는 deactivate되고 live key/connection은 revoke/disconnect된다.

## Agent API keys

```http
GET    /api/v1/agents/{agent_id}/keys?limit=50&cursor=...
POST   /api/v1/agents/{agent_id}/keys
POST   /api/v1/agents/{agent_id}/keys/{key_id}
DELETE /api/v1/agents/{agent_id}/keys/{key_id}
```

- Caller는 agent owner user여야 한다.
- Plaintext token은 create/rotation 응답에서 한 번만 반환한다.
- Agent account당 live key 최대 5개.
- `expires_at` 필수, 최대 365일.
- `scopes`는 빈 배열만 허용한다.
