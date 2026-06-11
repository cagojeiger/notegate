# REST Nodes

Node API는 Space tree의 node 속성을 다룬다. Content는 Text/File category에서 다룬다.

```http
GET    /api/v1/spaces/{space_id}/paths/resolve?path=/notes/state.json
GET    /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/children?limit=100&cursor=...
POST   /api/v1/spaces/{space_id}/nodes
PATCH  /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PUT    /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PATCH  /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
POST   /api/v1/spaces/{space_id}/nodes/{node_id}/move
DELETE /api/v1/spaces/{space_id}/nodes/{node_id}?recursive=true
```

## Response shapes

`children`과 MCP search 후보 목록은 `NodeTreeItem`을 반환한다. `GET /nodes/{node_id}`와 `files_stat` 성격의 조회는 `NodeDetail`을 반환한다.

```text
NodeTreeItem
  id
  parent_id
  name
  kind
  path
  sort_order
  has_children
  byte_len        # folder는 null, text/file은 저장 bytes 기준
  updated_at

NodeDetail = NodeTreeItem +
  metadata
  created_at
  created_by_account_id
  updated_by_account_id
  text_summary    # kind=text일 때만
  file_summary    # kind=file일 때만
```

`NodeTreeItem`은 tree 화면과 후보 목록용이므로 content body와 전체 metadata를 포함하지 않는다.

Node kind:

```text
folder
text
file
```

Create rules:

- Folder create는 `nodes(kind='folder')`만 만든다.
- Text create는 `nodes(kind='text')`와 `text_objects`를 함께 만든다.
- Text create 요청에 `content`가 있으면 plain Text content를 같은 요청에서 쓴다.
- Text create 요청에서는 encrypted payload를 받지 않는다. Encrypted Text는 Text API `PUT /text/{node_id}`로 저장한다.
- File node create는 REST node create에서 허용하지 않는다.
- 같은 parent 안 live name은 unique다.

## Node metadata

모든 node는 `metadata` JSON object를 가진다.

```json
{
  "title": "공급 계약 초안",
  "tags": ["contract", "legal"],
  "status": "draft"
}
```

Rules:

- `metadata`는 folder/text/file 공통 속성이다.
- `metadata`는 content가 아니며 Text/File 본문 암호화 대상이 아니다.
- 민감한 값은 `metadata`에 넣지 않는다.
- `metadata`는 JSON object만 허용한다.

Update rules:

- `GET /metadata`는 `{ "metadata": {...} }`를 반환한다.
- `PUT /metadata`는 `{ "metadata": {...} }`로 metadata 전체를 교체한다.
- `PATCH /metadata`는 `{ "patch": {...} }`로 JSON Merge Patch 방식 부분 수정을 수행한다.
- PATCH에서 `null` 값은 해당 key 삭제를 의미한다.
