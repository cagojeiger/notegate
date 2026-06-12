# REST Space agent connections

Connection은 owner user가 agent를 space에 연결하는 product resource다.

## List connected agents

```http
GET /api/v1/spaces/{space_id}/agents?limit=100&cursor=...
```

Owner user만 호출한다. 연결된 agent와 permission을 반환한다.

## Connect or update agent

```http
PUT /api/v1/spaces/{space_id}/agents/{agent_id}
```

```json
{"permission":"read"}
```

```json
{"permission":"write"}
```

규칙:

- Caller는 space owner user여야 한다.
- Agent는 caller가 소유한 active agent여야 한다.
- `write`는 `read`를 포함한다.
- Connection 생성/변경은 API key를 만들지 않는다.

## Disconnect agent

```http
DELETE /api/v1/spaces/{space_id}/agents/{agent_id}
```

Owner user만 가능하다. Connection은 disconnected 상태가 되고 agent는 해당 space에 접근할 수 없다.
