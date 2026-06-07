# notegate API architecture

이 문서는 notegate API의 상위 architecture와 surface 경계를 정의한다. 개별 endpoint의 request/response 계약은 `rest-api.md`와 `mcp-tools.md`에서 정의한다.

```text
Browser/UI REST API  = 화면과 tree state를 위한 node_id 중심 API
MCP tools            = LLM/터미널 사용성을 위한 path 중심 command API
```

REST와 MCP는 서로 다른 입력 방식을 쓰지만, 같은 workspace/account/access 규칙과 같은 DB를
사용한다.


## Document boundary

`api.md`와 `rest-api.md`는 역할이 다르다.

```text
api.md       = API architecture / category / layer / cross-surface invariant
rest-api.md  = HTTP REST endpoint contract / URL / request / response / status code
mcp-tools.md = MCP tool contract / path-oriented command API
```

`api.md`에는 개별 endpoint body를 자세히 쓰지 않는다. 대신 REST, MCP, Auth, System이
어떤 책임을 갖고 어떤 category/layer로 나뉘는지 정의한다.

`rest-api.md`에는 실제 HTTP 계약을 쓴다. frontend/BFF/API handler 구현자는 이 문서를
기준으로 route, DTO, status code, pagination을 맞춘다.

## 설계 원칙

1. DB는 workspace/account/file tree의 source of truth다.
2. REST는 브라우저 UI가 쓰기 쉬운 resource API를 제공한다.
3. MCP는 LLM이 터미널 명령처럼 쓰기 쉬운 path command API를 제공한다.
4. REST와 MCP는 DB를 직접 다루지 않고 공통 domain/service 계층을 호출한다.
5. 목록, 검색, 읽기, subtree 변경은 모두 limit/pagination/truncation 정책을 가진다.
6. 현재 검색은 Postgres `LIKE`/`ILIKE` 기반으로 단순하게 유지한다.
7. API는 기능 단위 category를 명확히 나눈다. `files` 하나에 identity/access/agent를 섞지 않는다.


## API categories

REST API는 다음 category로 나눈다. category는 URL, handler module, domain service의
경계를 정하는 기준이다.

```text
Auth       /auth/*, /.well-known/oauth-protected-resource*
Identity   /api/v1/me
Workspace  /api/v1/workspaces
Nodes      /api/v1/workspaces/{workspace_id}/nodes
Documents  /api/v1/workspaces/{workspace_id}/documents
Search     /api/v1/workspaces/{workspace_id}/search
Access     /api/v1/workspaces/{workspace_id}/access
Agents     /api/v1/agents
System     /health, /ready, /openapi.json, /swagger-ui
MCP        /mcp
```

분류 기준:

- auth/session resource: login, callback, logout, OAuth protected-resource metadata.
- workspace 밖 global resource: `me`, `workspaces`, `agents`, system endpoints.
- workspace 안 resource: `nodes`, `documents`, `search`, `access`.
- LLM/CLI command surface: REST category에 억지로 맞추지 않고 `/mcp` tool로 분리한다.
- `files`는 product concept이지 top-level REST category가 아니다. REST에서는
  `nodes`/`documents`/`search`로 나눈다.

## Implementation layers

구현도 API category와 같은 방향으로 나눈다.

```text
api/auth
api/rest/me
api/rest/workspaces
api/rest/nodes
api/rest/documents
api/rest/search
api/rest/access
api/rest/agents
api/mcp

domain/identity
domain/workspaces
domain/files
domain/search
domain/access
domain/agents

db/account_repo
db/workspace_repo
db/files_repo
db/agent_repo
```

레이어 규칙:

- Auth layer는 OAuth redirect/callback/session 발급과 bearer challenge metadata를 담당한다.
- API layer는 HTTP/MCP DTO, auth extraction, error mapping만 담당한다.
- Domain layer는 권한 체크, 파일 invariant, command semantics를 담당한다.
- DB layer는 query/transaction/pagination cursor 구현만 담당한다.
- Search가 커지면 `domain/search`와 `db/search_repo`를 별도 indexing backend로 교체할 수 있어야 한다.

## 문서 구성

- [`db.md`](db.md): 현재 사용하는 canonical 테이블 설계.
- [`files-commands.md`](files-commands.md): `ls`, `read`, `mv` 같은 공통 파일 명령의 의미.
- [`rest-api.md`](rest-api.md): HTTP REST endpoint 계약.
- [`mcp-tools.md`](mcp-tools.md): LLM/CLI 친화 MCP tool 계약.
- [`search.md`](search.md): `find`, `grep`의 현재 단순 검색 방식.
- [`performance-limits.md`](performance-limits.md): pagination, max size, subtree 제한 정책.

## Surface responsibilities

### REST

REST는 화면을 위한 API다. UI는 파일트리 node를 펼치고 선택 상태를 유지해야 하므로
`workspace_id + node_id` 중심 계약을 사용한다. `workspace_id`는 secret이 아니며
서버가 매 요청마다 `workspace_access`로 권한을 검증한다.

```text
me -> workspaces -> children(workspace_id, root_node_id) -> document(workspace_id, node_id)
```

`/api/v1/files/root` 같은 전역 root endpoint는 두지 않는다. root node id는 workspace
응답에 포함한다.

### MCP

MCP는 LLM과 CLI 감각을 위한 API다. 사용자는 보통 node UUID나 workspace UUID가 아니라
workspace 이름과 path를 말하므로 MCP file tool은 `workspace` name selector + path 중심 계약을 사용한다.

```text
workspaces_list()
files_ls(workspace, path)
files_read(workspace, path)
files_write(workspace, path, content_md)
files_patch(workspace, path, edits)
files_mv(workspace, source_path, destination_path)
```

MCP 내부 구현은 먼저 workspace name을 caller가 접근 가능한 workspace로 resolve하고, 선택된 workspace 안에서 path를 resolve한 뒤 기존 node/document primitive를 조합한다. 같은 이름의 접근 가능 workspace가 여러 개면 ambiguity error를 반환한다. `workspace_id`는 모호성 해소용 fallback으로만 허용한다.

## Identity mapping

인증 방식은 account kind를 결정한다.

```text
browser login via authgate       -> user account
MCP OAuth 2.1 via authgate        -> user account
device flow via authgate          -> user account
API key / agent key               -> agent account
```

OAuth 계열 인증은 사람 사용자를 증명하므로 항상 `user`로 처리한다.
API key는 장기/자동화 credential이므로 항상 `agent`로 처리한다.

## 공통 불변식

- 모든 작업은 인증된 account가 접근 권한을 가진 workspace 안에서만 실행한다.
- 클라이언트는 호출자 `user_id`나 `account_id`를 직접 보내지 않는다.
- REST 클라이언트는 URL에 `workspace_id`를 명시하고, 서버는 `workspace_access`로 권한을 검증한다.
- MCP는 path 중심으로 호출하되 내부에서 workspace context와 path를 resolve한다.
- root는 workspace마다 정확히 하나이며, workspace 생성 시 canonical root node `/`가 자동 생성된다. `parent_id = NULL`은 root에만 허용한다.
- workspace name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이다.
- root 외 node name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$` 형식이며 `/`, `:`, 공백, control character, `.`, `..`를 허용하지 않는다.
- folder name은 최대 `128` chars, document filename은 `.md` 포함 최대 `128` chars다.
- document title stem은 `.md` 제외 최대 `125` chars이며, 현재 제목은 filename stem으로 본다.
- 같은 parent folder 안에서 살아있는 node 이름은 unique하다.
- 다른 folder에서는 같은 이름을 사용할 수 있다.
- path는 `parent_id + name` tree에서 derive하며, path uniqueness는 sibling unique invariant로 보장한다.
- owner account당 workspace는 최대 `20`개다.
- workspace active access account는 최대 `20`개다.
- creator account당 active agent는 최대 `50`개, agent당 active key는 최대 `10`개다.
- 파일트리 최대 depth는 `5`, folder당 live direct children은 최대 `200`, workspace live nodes는 최대 `10000`개다.
- workspace live documents는 최대 `5000`개이고, live document 원문 총량은 최대 `268435456` bytes다.
- Markdown document 하나는 최대 `524288` bytes, `2000` lines다.
- document node 이름은 `.md`로 끝난다.
- folder node 이름은 `.md`로 끝날 수 없다.
- 삭제된 node는 `ls`, `find`, `grep`, `stat`, `read` 결과에서 보이지 않는다.
