# notegate Files surfaces

notegate의 파일 기능은 하나의 개인 Markdown 파일시스템을 두 개의 외부
surface로 노출한다.

```text
Browser/UI REST API  = 화면과 tree state를 위한 node_id 중심 API
MCP tools            = LLM/터미널 사용성을 위한 path 중심 command API
```

두 surface는 서로 다른 입력 방식을 쓰지만, 같은 파일트리 규칙과 같은 DB를
사용한다.

## 설계 원칙

1. DB는 파일트리의 source of truth다.
2. REST는 브라우저 UI가 쓰기 쉬운 node tree API를 제공한다.
3. MCP는 LLM이 터미널 명령처럼 쓰기 쉬운 path command API를 제공한다.
4. REST와 MCP는 DB를 직접 다루지 않고 공통 files command/service 계층을 호출한다.
5. 목록, 검색, 읽기, subtree 변경은 모두 limit/pagination/truncation 정책을 가진다.
6. 현재 검색은 Postgres `LIKE`/`ILIKE` 기반으로 단순하게 유지한다.

## 문서 구성

- [`db.md`](db.md): 현재 사용하는 canonical 테이블 설계.
- [`files-commands.md`](files-commands.md): `ls`, `cat`, `mv` 같은 공통 파일 명령의 의미.
- [`rest-api.md`](rest-api.md): UI용 REST API 계약.
- [`mcp-tools.md`](mcp-tools.md): LLM/CLI 친화 MCP tool 계약.
- [`search.md`](search.md): `find`, `grep`의 현재 단순 검색 방식.
- [`performance-limits.md`](performance-limits.md): pagination, max size, subtree 제한 정책.

## Surface 책임

### REST

REST는 화면을 위한 API다. UI는 파일트리 node를 펼치고 선택 상태를 유지해야 하므로
`node_id` 중심 계약을 사용한다.

```text
root -> children(node_id) -> document(node_id) -> save/move/delete(node_id)
```

### MCP

MCP는 LLM과 CLI 감각을 위한 API다. 사용자는 보통 UUID가 아니라 path를 말하므로
MCP tool은 path 중심 계약을 사용한다.

```text
files_ls(path)
files_cat(path)
files_write(path, content_md)
files_mv(source_path, destination_path)
```

MCP 내부 구현은 path를 resolve한 뒤 기존 node/document primitive를 조합한다.

## 공통 불변식

- 모든 작업은 인증된 사용자의 default workspace 안에서만 실행한다.
- 클라이언트는 `user_id`나 `workspace_id`를 직접 보내지 않는다.
- root는 workspace마다 정확히 하나이며, workspace 생성 시 canonical root node `/`가 자동 생성된다. `parent_id = NULL`은 root에만 허용한다.
- 같은 parent folder 안에서 살아있는 node 이름은 unique하다.
- 다른 folder에서는 같은 이름을 사용할 수 있다.
- 살아있는 node path는 workspace 안에서 unique하다.
- document node 이름은 `.md`로 끝난다.
- folder node 이름은 `.md`로 끝날 수 없다.
- 삭제된 node는 `ls`, `find`, `grep`, `stat`, `cat` 결과에서 보이지 않는다.
