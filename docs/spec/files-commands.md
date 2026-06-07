# Files command semantics

이 문서는 REST와 MCP가 공유하는 파일 명령의 의미를 정의한다. REST는 UI용
node_id API를 노출하고, MCP는 CLI-style path API를 노출하지만, 둘 다 이 command
semantics를 따른다.

## Path rules

- 모든 path는 `/`로 시작한다.
- root는 `/`다.
- root 외 trailing slash는 canonical path에 저장하지 않는다.
- 빈 segment, `.`, `..` segment는 허용하지 않는다.
- path는 server에서 canonical form으로 normalize한다.
- 최대 path 길이와 depth는 [`performance-limits.md`](performance-limits.md)를 따른다.

## Name rules

- root 외 node name은 빈 문자열일 수 없다.
- `/`, NUL, CR/LF를 포함할 수 없다.
- `.` 또는 `..`일 수 없다.
- document name은 `.md`로 끝나야 한다.
- folder name은 `.md`로 끝날 수 없다.
- 같은 parent folder 안의 살아있는 node는 같은 name을 가질 수 없다.

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
4. 같은 parent 안 name 충돌이 있으면 conflict다.
5. `nodes(kind='folder')`를 insert한다.

### `touch`

빈 Markdown document를 만든다.

CLI-style 입력:

```text
path = /projects/note.md
```

동작:

1. parent path를 resolve한다.
2. basename이 `.md`로 끝나는지 검증한다.
3. `nodes(kind='document')`와 `documents` row를 하나의 transaction에서 만든다.
4. 빈 문서도 `document_lines`/`document_index_status`를 일관되게 초기화한다.

### `cat` / `open`

Markdown 문서를 읽는다.

입력:

```text
path or node_id
start_line
max_lines
max_bytes
```

규칙:

- 대상은 document여야 한다.
- 큰 문서는 line/byte 제한으로 잘라 반환한다.
- 잘린 응답은 `truncated=true`, `next_start_line`을 포함한다.

### `write` / `save`

문서 전체 내용을 저장한다.

입력:

```text
path or node_id
content_md
create=false|true
```

규칙:

- `create=false`이면 대상 document가 존재해야 한다.
- `create=true`이면 없을 때 parent를 resolve한 뒤 새 document를 만들 수 있다.
- content size는 hard limit을 넘을 수 없다.
- 저장은 `documents`, `nodes.updated_at`, 검색 인덱스 갱신을 같은 logical operation으로 처리한다.

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
8. folder 이동 시 descendant `path_cache`도 함께 갱신한다.
9. subtree size limit을 넘는 동기 move는 거부한다.

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
path
kind
scope path
```

규칙:

- pagination 필수.
- 기본 scope는 `/`이지만, LLM/MCP는 좁은 scope를 선호해야 한다.

### `grep`

Markdown 본문을 line 단위로 검색한다.

검색 대상:

```text
document_lines.line_text
```

규칙:

- scope path를 지원한다.
- context line 수는 제한한다.
- pagination 필수.
- 결과 path는 `nodes.path_cache`에서 가져온 최신 path다.
