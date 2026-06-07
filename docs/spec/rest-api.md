# UI REST API

REST API는 브라우저 UI를 위한 node tree API다. UI는 tree node를 펼치고 선택 상태를
유지해야 하므로 `node_id` 중심 계약을 사용한다.

Base path:

```text
/api/v1/files
```

모든 endpoint는 인증된 사용자 default workspace 안에서만 동작한다.

## 공통 page shape

목록과 검색 응답은 page metadata를 포함한다.

```json
{
  "page": {
    "limit": 100,
    "returned": 100,
    "has_more": true,
    "next_cursor": "opaque-cursor"
  }
}
```

`cursor`는 opaque token이며 클라이언트가 해석하지 않는다.

## Node output

```json
{
  "id": "node-id",
  "parent_id": "parent-id-or-null",
  "name": "note.md",
  "kind": "document",
  "path": "/projects/note.md",
  "sort_order": 0,
  "has_children": false,
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```

## Endpoints

### Root

```http
GET /api/v1/files/root
```

Initializes the default workspace if needed and returns its canonical root node. Workspace creation creates the root node automatically.

### Resolve path

```http
GET /api/v1/files/resolve?path=/projects/note.md
```

UI command bar나 deep link가 path를 node로 바꿀 때 사용한다.

### Children

```http
GET /api/v1/files/nodes/{node_id}/children?limit=100&cursor=...
```

규칙:

- `{node_id}`는 folder여야 한다.
- 직접 자식만 반환한다.
- pagination 필수.

응답:

```json
{
  "parent": {"id": "folder-id", "path": "/projects"},
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false}
}
```

### Create folder

```http
POST /api/v1/files/folders
```

```json
{
  "parent_node_id": "folder-id",
  "name": "notes"
}
```

### Create document

```http
POST /api/v1/files/documents
```

```json
{
  "parent_node_id": "folder-id",
  "name": "note.md"
}
```

Creates an empty Markdown document.

### Open document

```http
GET /api/v1/files/documents/{node_id}?start_line=1&max_lines=200&max_bytes=65536
```

큰 문서는 truncated response를 반환할 수 있다.

### Save document

```http
PATCH /api/v1/files/documents/{node_id}
```

```json
{
  "content_md": "# Note\n"
}
```

전체 문서 replace 방식이다. content size hard limit을 넘으면 거부한다.

### Move node

```http
PATCH /api/v1/files/nodes/{node_id}/move
```

```json
{
  "new_parent_node_id": "folder-id",
  "new_name": "renamed.md"
}
```

`new_name`이 없으면 기존 이름을 유지한다.

### Delete node

```http
DELETE /api/v1/files/nodes/{node_id}?recursive=true
```

- document는 `recursive=false`로 삭제 가능하다.
- folder는 `recursive=true`가 필요하다.

### Find

```http
POST /api/v1/files/search/find
```

```json
{
  "q": "note",
  "path": "/projects",
  "kind": "document",
  "limit": 50
}
```

### Grep

```http
POST /api/v1/files/search/grep
```

```json
{
  "q": "auth",
  "path": "/projects",
  "context": 2,
  "limit": 20
}
```

## Error policy

- Missing/invalid auth: `401`
- Registered but inactive user: `403`
- Not found or cross-workspace access: `404`
- Invalid field/name/path/limit: `400`
- Root move/delete, duplicate destination, subtree too large: `409`
- Internal errors: `500` with redacted message
