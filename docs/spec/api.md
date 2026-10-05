# API 구조

사람과 AI Agent는 같은 Space tree와 service invariant를 사용한다.

## 문서 원칙

현재 runtime 계약만 기록한다. Route, schema, 상수와 DB 제약의 정본은 코드와 migration이며 계약 변경 시 문서도 함께 맞춘다. 지원 동작을 먼저, 오류·복구·상한을 뒤에 적는다. ADR도 현재 유효한 구조와 결정만 기록한다. 변경 연혁·완료 보고·미구현 계획은 문서에 넣지 않고 issue와 Git history에서 관리한다.

| 정본 | 소유하는 내용 |
|---|---|
| [Domain](domain.md) | 제품 용어, 소유권, permission |
| [File tree commands](files-commands.md) | Path, 공통 command, write lock |
| [Security](security.md) | 인증·키·암호화 |
| [Lifecycle](lifecycle.md) | 삭제·보존 |
| [Performance limits](performance-limits.md) | 전역 상한 |
| [Search](search.md) | 순회, scan budget, cache |
| [DB](db.md) / [Schemas](schemas.md) | 저장 제약 / 공통 응답 형태 |
| [개발 가이드](../development.md) | 배포 환경 변수, object storage, CORS |

Surface 문서는 request/response와 오류·차이만 정의하고 공통 계약은 정본에 연결한다.

## API 분류

| Surface | Route | 대상·계약 |
|---|---|---|
| Auth | `/auth/*`, `/.well-known/*` | Login/OAuth |
| [Web V1](rest/README.md) | `/api/v1/*` | 브라우저 전체 resource API |
| [Public V2](public-api-v2.md) | `/api/v2/*` | 안정적인 외부 ID 기반 API; Agent MCP와 동등한 Space 내부 기능 |
| [Command](command-api.md) | `POST /cli` | CLI의 Space name + path 기반 HTTP |
| [MCP](mcp/README.md) | `/mcp`, `/mcp/v2` | 같은 command의 JSON-RPC adapter |
| System | `/health`, `/ready` | Health/readiness |
| API Docs | `/openapi/v2.json`, `/swagger-ui/v2` | V2 공개 schema |

## 계층

```text
api/auth/*      transport 인증과 credential extraction
api/rest/*      Web V1 request/response와 DTO mapping
api/public_v2   공개 계약용 request/response와 DTO mapping
api/command_api HTTP command request/response와 error mapping
api/mcp/*       MCP tool schema, protocol envelope와 command adapter
api/commands/*  인증된 caller 기반 path-first command 실행과 검증
command/*       transport-neutral command input, recovery와 error 계약
search/*        find/grep pipeline, matcher, body cache, search telemetry
service/*       authorization, limits, lifecycle invariant
repo/db         transaction, SQL, DB constraint mapping
model           shared domain types
```

MCP는 인증된 request에서 context를 한 번 만들고 공통 command 실행기를 호출한다. 직접 tool과 sequence는 같은 검증·실행·recovery/error 계약을 사용한다. API layer는 envelope/mapping을, service/model은 업무 규칙을 소유한다. V2를 별도 process로 나누어도 이 경계는 유지한다.

## Identity mapping

AuthGate browser login·MCP OAuth·device flow는 User, `ngk_v2_` key는 active Agent account로 resolve한다. Browser는 opaque session cookie를 사용하고 BE가 암호화 저장한 refresh token으로 갱신한다.

| Route | 허용 credential |
|---|---|
| `/api/v1/*` | Browser session cookie |
| `/api/v2/*` | Agent `ngk_v2_` API key |
| `/cli` | CLI audience User OAuth 또는 Agent `ngk_v2_` key |
| `/mcp` | User MCP OAuth bearer |
| `/mcp/v2` | Agent `ngk_v2_` API key |

Caller `user_id`/`account_id`는 client 입력으로 받지 않는다. Command API와 두 MCP endpoint는 인증·transport·운영 한계를 분리하며 command 동작을 복제하지 않는다.

## Common invariants

공통 규칙은 [Domain](domain.md), [File tree commands](files-commands.md), [Security](security.md), [DB](db.md)를 따른다. 특히 Node metadata와 Markdown frontmatter는 별개이고, 서버 at-rest 암호화와 client-side 암호화도 구분한다. File bytes는 API JSON이 아니라 presigned URL로 직접 전송한다. Action attribution은 account id로 기록한다.
