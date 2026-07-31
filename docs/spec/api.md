# API 구조

NoteGate API는 사람과 AI agent가 같은 Space tree를 다루도록 한다. V1은 브라우저 UI 전용 resource API, V2는 외부 확장용 API key API, MCP는 agent/CLI용 path-first command API다.

```text
V1 REST  = 브라우저 UI가 사용하는 전체 resource API
V2 REST  = 외부 확장이 사용하는 안정적인 공개 API
MCP      = agent가 쓰기 쉬운 space name + path 기반 command/search API
```

세 surface는 같은 service invariant를 사용한다. V2 초기 계약은 API key caller 확인만 제공한다.

## API 분류

```text
Auth        /auth/*, /.well-known/*
Web API     /api/v1/*
Public API  /api/v2/*
System      /health, /ready
API Docs    /openapi/v2.json, /swagger-ui/v2
MCP         /mcp
```

V2의 초기 endpoint는 `public-api-v2.md`에서 정의한다.

## 계층

```text
api/auth/*      transport 인증과 credential extraction
api/rest/*      Web V1 request/response와 DTO mapping
api/public_v2   공개 계약용 request/response와 DTO mapping
api/mcp/*       tool schema, space/path resolve, DTO mapping
service/*       authorization, limits, lifecycle invariant
repo/db         transaction, SQL, DB constraint mapping
model           shared domain types
```

API layer는 space/text/file/agent 업무 규칙을 직접 구현하지 않는다. V2가 별도 프로세스로 분리되더라도 같은 service/model 계약을 사용한다.

## Identity mapping

```text
browser login via authgate -> user account
MCP OAuth via authgate      -> user account
device flow via authgate    -> user account
ngk_v1_ API key             -> api_keys.account_id account
```

OAuth 계열 인증은 user로 처리한다. Browser login은 opaque browser session cookie를 발급하고, BE가 저장한 encrypted authgate refresh token으로 server-side 갱신한다. API key는 `api_keys.account_id`가 가리키는 account kind로 caller를 결정한다.

```text
/api/v1/* -> browser session cookie만 허용
/api/v2/* -> ngk_v1_ API key만 허용
/mcp      -> ngk_v1_ API key 또는 MCP OAuth bearer 허용
```

## Common invariants

- 클라이언트는 caller `user_id`/`account_id`를 직접 보내지 않는다.
- User는 자신이 소유한 space를 관리한다.
- Agent는 연결된 space에서만 permission에 따라 작업한다.
- Space 안 tree source of truth는 `parent_id + name`이다. Full path는 저장하지 않고 derive한다.
- Space마다 root node `/`가 하나 있다.
- Node kind는 `folder`, `text`, `file` 중 하나다.
- Node는 folder/text/file 공통 `metadata` JSON object를 가진다.
- Text는 plain UTF-8 content 또는 client-side encrypted payload다. `system_max` Space는 plain Text를 서버 관리 방식으로 at-rest 암호화할 수 있다. grep/patch는 plain Text만 대상으로 하며 서버 관리 암호화는 서버에서 투명하게 복호화한다.
- Markdown Text의 leading YAML frontmatter는 Text content 안의 표시용 convention이며 Node `metadata`로 해석하거나 동기화하지 않는다.
- File은 object/binary content다. REST와 MCP `file_transfer`는 S3 호환 presigned URL을 제공하며, File bytes는 API JSON payload를 통과하지 않는다.
- Agent connection permission은 `read` 또는 `write`다. `write`는 `read`를 포함한다.
- User/agent action attribution은 account id로 기록한다.
