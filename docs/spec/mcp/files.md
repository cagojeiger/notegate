# MCP Files

## `files_ls`

폴더의 live direct children을 반환한다.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects",
  "limit": 100,
  "cursor": "opaque-cursor"
}
```

첫 page에서는 `cursor`를 생략하고, 이후 page는 이전 응답의 `page.next_cursor`를 그대로 다시 전달한다.

Output:

```json
{
  "workspace": "personal",
  "path": "/projects",
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false, "next_cursor": null}
}
```

## `files_stat`

경로 하나의 메타데이터를 반환한다. Document node는 `content_sha256`, `byte_len`, `line_count`를 포함하고 folder node는 이 필드를 생략한다.

Input:

```json
{"workspace": "personal", "path": "/projects/note.md"}
```

Output:

```json
{
  "workspace": "personal",
  "node": {
    "path": "/projects/note.md",
    "name": "note.md",
    "kind": "document",
    "node_id": "node-id",
    "has_children": false,
    "sort_order": 0,
    "created_at": "2026-06-08T00:00:00Z",
    "updated_at": "2026-06-08T00:00:00Z",
    "content_sha256": "sha256...",
    "byte_len": 7,
    "line_count": 1
  }
}
```

## `files_mkdir`

경로에 folder node를 생성한다.

Input:

```json
{"workspace": "personal", "path": "/projects/notes"}
```

CLI 의도:

```sh
mkdir /projects/notes
```

Output:

```json
{
  "workspace": "personal",
  "node": {
    "path": "/projects/notes",
    "name": "notes",
    "kind": "folder",
    "node_id": "node-id",
    "has_children": false,
    "sort_order": 0,
    "created_at": "2026-06-08T00:00:00Z",
    "updated_at": "2026-06-08T00:00:00Z"
  }
}
```

## `files_touch`

빈 Markdown document를 생성한다.

Input:

```json
{"workspace": "personal", "path": "/projects/note.md"}
```

CLI 의도:

```sh
touch /projects/note.md
```

Output:

```json
{
  "workspace": "personal",
  "node": {
    "path": "/projects/note.md",
    "name": "note.md",
    "kind": "document",
    "node_id": "node-id",
    "has_children": false,
    "sort_order": 0,
    "created_at": "2026-06-08T00:00:00Z",
    "updated_at": "2026-06-08T00:00:00Z",
    "content_sha256": "sha256...",
    "byte_len": 0,
    "line_count": 0
  },
  "content_sha256": "sha256...",
  "byte_len": 0,
  "line_count": 0
}
```

## `files_read`

Markdown document를 제한된 line/byte range로 읽는다.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "start_line": 1,
  "max_lines": 200,
  "max_bytes": 65536,
  "if_none_match_sha256": "optional"
}
```

Output:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "content_md": "# Note\n",
  "content_sha256": "sha256...",
  "byte_len": 7,
  "line_count": 1,
  "start_line": 1,
  "end_line": 1,
  "returned_lines": 1,
  "truncated": false,
  "next_start_line": null
}
```

`if_none_match_sha256` 분기:

```text
없거나 현재 hash와 다름 -> 제한된 content 반환
현재 hash와 같음        -> content 없이 metadata만 반환
```

변경 없음 output:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "unchanged": true,
  "content_returned": false,
  "content_sha256": "sha256..."
}
```

## Mutation safety contract

`files_write`와 `files_patch`는 변경 tool이다. 대상이 stale 상태이거나, 매칭이 모호하거나, 안전하게 수정할 수 없으면 실패한다.

공통 변경 계약:

```text
document/node 작업 1회 -> transaction 1개
expected_sha256 불일치 -> 변경 전 conflict
변경 성공             -> content_sha256, byte_len, line_count, current path 반환
변경 실패             -> partial persistence 없음
저장된 hash 불일치    -> 성공이 아니라 internal error
```

오류 메시지는 `read the document again`, `old_text matched multiple times`, `use files_write for full rewrite`처럼 caller가 다음 행동을 정할 수 있는 hint를 포함한다.

`files_patch`는 fuzzy matching이 아니라 exact matching을 사용한다. 그래서 MCP document edit은 예측 가능하고 추적 가능해야 한다.

## `files_write`

Markdown document 전체 내용을 교체한다.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "content_md": "# Updated\n",
  "create": false,
  "expected_sha256": "optional"
}
```

Rules:

- `create=false`는 기존 document가 있어야 한다.
- `create=true`는 missing document를 `dirname(path)` 아래에 생성한다.
- content가 `524288` bytes 또는 `2000` lines를 넘으면 문서 분리 hint와 함께 거부한다.
- 새 document 생성 후 workspace live documents가 `5000`개를 넘으면 거부한다.
- write 후 workspace live document content가 `268435456` bytes를 넘으면 거부한다.
- `expected_sha256`가 있고 현재 document와 다르면 conflict를 반환한다.
- 성공한 write는 새 `content_sha256`, `byte_len`, `line_count`를 반환한다.

Output:

```json
{
  "workspace": "personal",
  "node": {
    "path": "/projects/note.md",
    "name": "note.md",
    "kind": "document",
    "node_id": "node-id",
    "has_children": false,
    "sort_order": 0,
    "created_at": "2026-06-08T00:00:00Z",
    "updated_at": "2026-06-08T00:00:00Z",
    "content_sha256": "sha256...",
    "byte_len": 10,
    "line_count": 1
  },
  "content_sha256": "sha256...",
  "byte_len": 10,
  "line_count": 1
}
```

## `files_patch`

Markdown document 하나에 exact targeted replacement를 적용한다.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "edits": [
    {
      "old_text": "before",
      "new_text": "after"
    }
  ],
  "expected_sha256": "optional"
}
```

Rules:

- `edits`는 비어 있으면 안 된다.
- `old_text`는 비어 있으면 안 된다.
- `old_text`와 `new_text`가 같으면 no-op patch로 보고 거부한다.
- 각 `old_text`는 원본 document에서 정확히 한 번만 매칭되어야 한다.
- matching은 저장된 Markdown text에 대해 exact로 수행한다. fuzzy, whitespace-normalized, Unicode-normalized, case-insensitive matching은 하지 않는다.
- `old_text` 매칭이 0개면 conflict를 반환하고 현재 document를 다시 읽거나 검색하라고 안내한다.
- `old_text` 매칭이 여러 개면 conflict를 반환하고 더 많은 surrounding context를 요구한다.
- 여러 edit은 이전 edit 결과가 아니라 원본 document 기준으로 매칭한다.
- 겹치거나 중첩된 edit range는 거부한다.
- 모든 edit은 atomic하게 적용한다. 하나라도 invalid이면 아무것도 저장하지 않는다.
- line ending은 substring replacement 결과를 따른다. patch 중 서버가 전체 line ending을 normalize하지 않는다.
- 결과 content가 `524288` bytes 또는 `2000` lines를 넘으면 거부한다.
- patch 후 workspace live document content가 `268435456` bytes를 넘으면 거부한다.
- 전체 rewrite이거나 안정적인 unique anchor를 제공하기 어렵다면 `files_write`를 사용한다.
- `expected_sha256`가 있고 현재 document와 다르면 matching 전에 conflict를 반환한다.

Output:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "patched": true,
  "edits_applied": 1,
  "content_sha256": "sha256...",
  "previous_sha256": "sha256...",
  "byte_len": 5,
  "line_count": 1,
  "diff": "--- before\n+++ after\n..."
}
```

## `files_mv`

같은 workspace 안에서 경로를 이동하거나 이름을 바꾼다.

Input:

```json
{
  "workspace": "personal",
  "source_path": "/projects/note.md",
  "destination_path": "/archive/note.md"
}
```

Rules:

- `source_path == destination_path`이면 no-op success다.
- destination parent가 존재하고 folder이면 진행한다. 아니면 conflict/not_found다.
- destination에 같은 이름의 live sibling이 있으면 conflict다.
- folder를 자기 자신 또는 descendant 아래로 이동하는 것은 conflict다.
- folder move/rename은 이동한 node만 갱신한다. descendant path는 parent chain에서 derive한다.

Output:

```json
{
  "workspace": "personal",
  "node": {
    "path": "/archive/note.md",
    "name": "note.md",
    "kind": "document",
    "node_id": "node-id",
    "has_children": false,
    "sort_order": 0,
    "created_at": "2026-06-08T00:00:00Z",
    "updated_at": "2026-06-08T00:00:00Z",
    "content_sha256": "sha256...",
    "byte_len": 10,
    "line_count": 1
  }
}
```

## `files_rm`

경로를 삭제한다. 삭제된 node는 일반 tool에서 즉시 숨겨지고, purge 전까지 내부적으로 보존된다.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/old",
  "recursive": true
}
```

Output:

```json
{
  "workspace": "personal",
  "path": "/projects/old",
  "node_id": "deleted-root-node-id",
  "deleted": true,
  "purge_after": "2026-07-08T00:00:00Z"
}
```

Rules:

- Folder 삭제는 `recursive=true`가 필요하다.
- Root 삭제는 금지한다.
- 동기 삭제 한도를 넘는 subtree는 범위를 좁히라는 hint와 함께 거부한다.
- 삭제된 node는 현재 MCP contract에서 복구할 수 없다.
- `purge_after`는 내부 purge job이 row를 hard-delete할 수 있는 가장 이른 시각이다.
