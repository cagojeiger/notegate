# REST Agents

## Agents

Agent API는 agent account와 API key를 관리하는 user-only endpoint다. Agent의 workspace별 권한은 Access category에서 따로 부여한다. API key는 인증 시 `agent` account로 처리한다. Agent key lifecycle은 workspace role이 아니라 agent 생성자/소유자 규칙으로 관리한다. Workspace owner는 agent account에 workspace access를 grant/revoke할 수 있을 뿐, agent key를 관리하지 않는다.

Agent caller는 이 category를 호출할 수 없다. Agent는 `/api/v1/me`로 자기 identity를 확인하고, workspace owner는 Access category로 workspace별 agent access를 확인한다.

### List agents

```http
GET /api/v1/agents?limit=100&cursor=...
```

User caller가 생성한 active agent 목록을 반환한다. Default/max limit은 `100`이다.

### Create agent

```http
POST /api/v1/agents
```

```json
{
  "name": "research-agent"
}
```

`agent` account를 생성한다. Workspace 접근 권한은 workspace access API로 별도 부여한다. 하나의 creator account는 최대 `50`개의 active agent를 가질 수 있다.

### Delete agent

```http
DELETE /api/v1/agents/{agent_id}
```

Agent의 underlying account를 soft-deactivate하고, revoke되지 않은 key와 workspace access를 revoke한다.

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

평문 key는 생성 응답에서 정확히 한 번만 반환한다.

Live key는 다음 조건을 모두 만족한다.

```text
agent_keys.revoked_at IS NULL
agent_keys.expires_at IS NULL OR agent_keys.expires_at > now()
```

Branching 규칙:

```text
live keys < 10             -> key 생성
live keys >= 10            -> 409 conflict
scopes omitted or []       -> 허용
scopes non-empty           -> 400 invalid input
expires_at omitted/future  -> 허용
expires_at past or now     -> 400 invalid input
```

### Revoke agent key

```http
DELETE /api/v1/agents/{agent_id}/keys/{key_id}
```

대상 key에 `revoked_at`/`revoked_by`를 설정한다.
