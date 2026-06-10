# REST Agents

## Agents

Agent API는 agent account와 agent-bound API key를 관리하는 user-only endpoint다. Agent의 workspace별 권한은 Access category에서 따로 부여한다. Agent-bound API key는 인증 시 해당 `agent` account로 처리한다. Key lifecycle은 workspace role이 아니라 agent 생성자/소유자 규칙으로 관리한다. Workspace owner는 agent account에 viewer/editor workspace access를 grant/revoke할 수 있을 뿐, agent-bound API key를 관리하지 않는다. Agent account는 owner role을 받을 수 없다.

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

`agent` account를 생성한다. Agent 생성은 key나 workspace access를 자동 생성하지 않는다. 상세 lifecycle은 `docs/spec/lifecycle.md`를 따른다. 하나의 user creator account는 최대 `50`개의 active agent를 가질 수 있다.

### Delete agent

```http
DELETE /api/v1/agents/{agent_id}
```

Agent 삭제 side effect는 `docs/spec/lifecycle.md`의 Agent 삭제 정책을 따른다.

### List agent API keys

```http
GET /api/v1/agents/{agent_id}/keys?limit=50&cursor=...
```

Caller가 생성한 active agent의 API key metadata를 keyset pagination으로 반환한다. Live/revoked/expired metadata는 조회 가능하지만 평문 token은 반환하지 않는다. 응답은 `keys`와 공통 `page`를 포함한다.

### Create agent API key

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

Agent API key는 명시 호출로만 생성한다. Agent account는 동시에 최대 5개의 live API key를 가질 수 있다. 평문 key는 생성 응답에서 정확히 한 번만 반환하고 저장하지 않는다. DB에는 통합 `api_keys` row로 저장하며, `account_id`는 대상 agent account다. 상세 lifecycle은 `docs/spec/lifecycle.md`를 따른다.

Live key는 다음 조건을 모두 만족한다.

```text
api_keys.account_id = agent_id
api_keys.revoked_at IS NULL
api_keys.expires_at IS NULL OR api_keys.expires_at > now()
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

### Rotate agent API key

```http
POST /api/v1/agents/{agent_id}/keys/{key_id}
```

같은 agent account에 new key를 만들고 old key를 revoke한다. New plaintext token은 응답에서 정확히 한 번만 반환한다.

### Revoke agent API key

```http
DELETE /api/v1/agents/{agent_id}/keys/{key_id}
```

대상 key에 `revoked_at`/`revoked_by`를 설정한다. `revoked_reason`은 rotation/system revoke처럼 사유가 있는 경우에만 설정한다.
