# Files command semantics

이 문서는 REST와 MCP가 공유하는 파일 명령의 의미를 정의한다. REST는 UI용
node_id API를 노출하고, MCP는 CLI-style path API를 노출하지만, 둘 다 이 command
semantics를 따른다.

## Path rules

- 모든 path는 `/`로 시작한다.
- root는 `/`이고 workspace마다 실제 `nodes` row로 존재한다.
- root 외 trailing slash는 canonical path로 인정하지 않는다.
- 빈 segment, `.`, `..` segment는 허용하지 않는다.
- path는 server에서 canonical form으로 normalize한다.
- MCP/CLI 표시에서는 `workspace:/path/to/file.md` 축약 표기를 사용할 수 있다.
- DB source of truth는 full path string이 아니라 `parent_id + name` tree다. path는 parent chain에서 derive한다.
- 최대 path 길이와 depth는 [`performance-limits.md`](performance-limits.md)를 따른다. 현재 최대 depth는 `5`다.

## Name rules

- workspace name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이다.
- root name은 `/`로 고정한다. root 외 node name은 빈 문자열일 수 없다.
- root 외 node name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$` 형식이다.
- folder name은 최대 `128` chars다.
- document filename은 `.md` 포함 최대 `128` chars다.
- document title stem은 `.md` 제외 최대 `125` chars다. 현재 제목은 별도 컬럼이 아니라 filename stem이다.
- `/`, `:`, 공백, control character는 workspace/node name에 들어갈 수 없다.
- node name은 `.` 또는 `..`일 수 없다.
- document name은 `.md`로 끝나야 한다.
- folder name은 `.md`로 끝날 수 없다.
- 같은 parent folder 안의 살아있는 node는 같은 name을 가질 수 없다.
- 같은 parent folder 안 live direct children은 최대 `200`개다.
- workspace 안 live nodes는 최대 `10000`개다.
- workspace 안 live documents는 최대 `5000`개다.
- workspace 안 live document 원문 총량은 최대 `268435456` bytes다.
- Unicode 이름은 초기 설계에서 제외한다. 필요하면 normalization/collation 정책을 별도 결정한다.

## Commands

### `ls`

폴더의 직접 자식을 반환한다.

입력:

```text
path or node_id
limit
cursor
```

규칙:

- 대상은 folder여야 한다.
- 직접 자식만 반환한다.
- 결과는 `sort_order, name, id` 순서다.
- pagination은 필수다.

### `stat`

path 또는 node_id의 metadata를 반환한다.

출력:

```text
id, parent_id, name, kind, path, has_children, byte_len?, line_count?, created_at, updated_at
```

### `mkdir`

새 folder를 만든다.

CLI-style 입력:

```text
path = /projects/notes
```

동작:

1. `dirname(path)`를 parent path로 resolve한다.
2. `basename(path)`를 folder name으로 검증한다.
3. parent가 folder인지 확인한다.
4. resulting depth가 `5` 이하인지 확인한다.
5. parent의 live direct children 수가 `200` 미만인지 확인한다.
6. workspace live nodes가 `10000` 미만인지 확인한다.
7. 같은 parent 안 name 충돌이 있으면 conflict다.
8. `nodes(kind='folder')`를 insert한다.

### `touch`

빈 Markdown document를 만든다.

CLI-style 입력:

```text
path = /projects/note.md
```

동작:

1. parent path를 resolve한다.
2. basename이 `.md`로 끝나는지 검증한다.
3. resulting depth가 `5` 이하인지 확인한다.
4. parent의 live direct children 수가 `200` 미만인지 확인한다.
5. workspace live nodes가 `10000` 미만인지 확인한다.
6. workspace live documents가 `5000` 미만인지 확인한다.
7. `nodes(kind='document')`와 `documents` row를 하나의 transaction에서 만든다.
8. 빈 문서 원본 row를 만들고 `content_sha256`, `byte_len`, `line_count` 기본값을 유지한다.

### `read` / `open`

Markdown 문서를 읽는다.

입력:

```text
path or node_id
start_line
max_lines
max_bytes
if_none_match_sha256
```

규칙:

- 대상은 document여야 한다.
- 큰 문서는 line/byte 제한으로 잘라 반환한다.
- 잘린 응답은 `truncated=true`, `next_start_line`을 포함한다.
- `if_none_match_sha256`가 현재 content hash와 같으면 content 없이 unchanged 응답을 반환할 수 있다.

### `write` / `save`

문서 전체 내용을 저장한다.

입력:

```text
path or node_id
content_md
create=false|true
expected_sha256
```

규칙:

- `create=false`이면 대상 document가 존재해야 한다.
- `create=true`이면 없을 때 parent를 resolve한 뒤 새 document를 만들 수 있다. 이때 workspace live nodes `10000`, live documents `5000`, parent children `200`, depth/path/name 제한을 모두 검사한다.
- content size와 line count는 hard limit을 넘을 수 없다. 현재 최대 `524288` bytes, `2000` lines다.
- 저장 후 workspace live document 원문 총량이 `268435456` bytes를 넘으면 거부한다.
- `expected_sha256`가 현재 content hash와 다르면 conflict로 거부한다.
- 저장은 `documents`, `nodes.updated_at`, 검색 인덱스 갱신을 같은 logical operation으로 처리한다.
- 초과 문서는 저장하지 않고 사용자가 문서를 나누도록 conflict/validation hint를 반환한다.

### `patch`

문서 한 개에 정확한 텍스트 치환을 적용한다.

입력:

```text
path or node_id
edits[].old_text
edits[].new_text
expected_sha256
```

규칙:

- `edits`는 비어 있을 수 없다.
- `old_text`는 비어 있을 수 없다.
- `old_text`와 `new_text`가 같으면 no-op patch로 보고 거부한다.
- 각 `old_text`는 원본 문서에서 정확히 한 번만 매칭되어야 한다.
- 매칭은 저장된 Markdown 원문에 대한 exact match다. fuzzy, whitespace-normalized, Unicode-normalized, case-insensitive match는 하지 않는다.
- `old_text`가 0번 매칭되면 conflict로 거부하고 re-read/search 힌트를 준다.
- `old_text`가 여러 번 매칭되면 conflict로 거부하고 더 긴 주변 context를 요구한다.
- 여러 edit는 순차 결과가 아니라 같은 원본 문서를 기준으로 매칭한다.
- 겹치거나 중첩된 edit range는 거부한다.
- 모든 edit는 atomic하게 적용한다. 하나라도 실패하면 아무 변경도 저장하지 않는다.
- patch 중 line ending은 전역 normalize하지 않고 substring replacement로 보존한다.
- 전체 재작성 또는 안정적인 unique anchor가 어려운 경우는 `write`를 사용한다.
- 성공 응답은 새 `content_sha256`, `byte_len`, `line_count`, diff/summary를 포함한다.

### `mv`

node를 이동하거나 이름을 바꾼다.

CLI-style 입력:

```text
source_path
destination_path
```

동작:

1. source path를 resolve한다.
2. destination dirname을 parent path로 resolve한다.
3. destination basename을 최종 name으로 검증한다.
4. root 이동은 금지한다.
5. destination parent는 folder여야 한다.
6. 자기 자신이나 descendant 아래로 이동할 수 없다.
7. destination parent 안 같은 name이 있으면 conflict다.
8. resulting subtree depth가 `5`를 넘으면 conflict다.
9. move/rename은 moved node의 `parent_id`/`name`만 변경하고 descendant path rewrite를 하지 않는다.

`source_path == destination_path`는 no-op success로 처리한다.

### `rm`

node를 soft delete한다.

입력:

```text
path or node_id
recursive=false|true
```

규칙:

- root 삭제는 금지한다.
- document 삭제는 `recursive=false`로 가능하다.
- folder 삭제는 `recursive=true`가 필요하다.
- subtree size limit을 넘는 동기 delete는 거부한다.
- 삭제된 node는 목록/검색/resolve에서 보이지 않는다.

### `find`

node metadata를 검색한다.

검색 대상:

```text
name
kind
scope path
```

`find` query는 node name에 매칭한다. path는 결과 display와 scope 제한에 사용하며, path substring
검색은 현재 기본 `find` 계약에 포함하지 않는다.

규칙:

- pagination 필수.
- 기본 scope는 `/`이지만, LLM/MCP는 좁은 scope를 선호해야 한다.

### `grep`

Markdown 본문을 검색한다. 후보 문서는 `documents.content_md ILIKE`로 찾고, line number/context는 application code에서 만든다.

검색 대상:

```text
documents.content_md 후보 검색 후 application code에서 line-split
```

규칙:

- scope path를 지원한다.
- context line 수는 제한한다.
- pagination 필수.
- 결과 path는 parent chain에서 derive한 최신 path다.
