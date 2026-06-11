# API architecture

notegate API는 사람과 AI agent가 같은 Space tree를 다루도록 한다. REST는 브라우저/UI용 resource API이고, MCP는 agent/CLI용 path-first command API다.

```text
REST API = UI가 안정적으로 선택한 id 기반 resource API
MCP tools = agent가 쓰기 쉬운 space name + path 기반 command API
```

두 surface는 같은 service invariant를 사용한다.

## API categories

```text
Auth        /auth/*, /.well-known/*
Identity    /api/v1/me, /api/v1/me/keys
Spaces      /api/v1/spaces
Nodes       /api/v1/spaces/{space_id}/nodes
Text        /api/v1/spaces/{space_id}/text
Search      /api/v1/spaces/{space_id}/search
Agents      /api/v1/agents
Connections /api/v1/spaces/{space_id}/agents
System      /health, /ready
API Docs    /openapi.json, /swagger-ui
MCP         /mcp
```

## Layering

```text
api/rest/*     request/response, auth extraction, DTO mapping
api/mcp/*      tool schema, space/path resolve, DTO mapping
service/*      authorization, limits, lifecycle invariant
repo/db        transaction, SQL, DB constraint mapping
model          shared domain types
```

API layer는 space/text/file/agent 업무 규칙을 직접 구현하지 않는다.

## Identity mapping

```text
browser login via authgate -> user account
MCP OAuth via authgate      -> user account
device flow via authgate    -> user account
ngk_v1_ API key             -> api_keys.account_id account
```

OAuth 계열 인증은 user로 처리한다. API key는 `api_keys.account_id`가 가리키는 account kind로 caller를 결정한다.

## Common invariants

- 클라이언트는 caller `user_id`/`account_id`를 직접 보내지 않는다.
- User는 자신이 소유한 space를 관리한다.
- Agent는 연결된 space에서만 permission에 따라 작업한다.
- Space 안 tree source of truth는 `parent_id + name`이다. Full path는 저장하지 않고 derive한다.
- Space마다 root node `/`가 하나 있다.
- Node kind는 `folder`, `text`, `file` 중 하나다.
- Text는 UTF-8 content이며 read/write/patch/grep 가능하다.
- File은 object/binary content이며 REST/MCP file upload/download surface에 포함하지 않는다.
- Agent connection permission은 `read` 또는 `write`다. `write`는 `read`를 포함한다.
- User/agent action attribution은 account id로 기록한다.
