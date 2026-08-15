# MCP Identity

## `me`

Caller identity와 전역 capability, 현재 실행 중인 NoteGate 서버 버전을 반환한다. Space 목록은 `read` tool의 `op=spaces`로 조회한다.

User caller:

```json
{
  "account": {"id":"account-id","kind":"user","display_name":"Kang"},
  "user": {"email":"user@example.com"},
  "capabilities": {"can_create_space":true,"can_manage_agents":true},
  "server_version": "<running-version>"
}
```

Agent caller:

```json
{
  "account": {"id":"account-id","kind":"agent","display_name":"research-agent"},
  "agent": {"name":"research-agent"},
  "capabilities": {"can_create_space":false,"can_manage_agents":false},
  "server_version": "<running-version>"
}
```

`server_version`은 실행 중인 바이너리의 Cargo package version이다. 최신 MCP 응답의 `_meta.io.modelcontextprotocol/serverInfo.version`과 같은 값을 사용한다. 구형 client에서는 `initialize.serverInfo.version`으로도 같은 값을 확인할 수 있다.
