# REST Nodes

Node API는 Space tree의 node 속성을 다룬다. Content body는 Text/File category에서 다룬다.

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

공통 schema는 `../schemas.md`를 따른다.

```ts
GET /paths/resolve              -> RestNode
GET /nodes/{node_id}            -> RestNode
GET /nodes/{node_id}/children   -> { parent: NodeRef, children: RestNode[], page: Page }
POST /nodes                     -> RestNode
PATCH /nodes/{node_id}          -> RestNode
PUT/PATCH /nodes/{id}/metadata  -> RestNode
POST /nodes/{node_id}/move      -> RestNode
DELETE /nodes/{node_id}         -> 204 No Content
```

`RestNode`는 UI용 resource shape이므로 metadata, attribution, text/file summary를 포함한다. Text/File content body는 포함하지 않는다.

## Create rules

```ts
type CreateNodeBody = {
  parent_id: string
  kind: "folder" | "text" | "file"
  name: string
  content?: string
}
```

- Folder create는 `nodes(kind='folder')`만 만든다.
- Text create는 `nodes(kind='text')`와 `text_objects`를 함께 만든다.
- Text create 요청에 `content`가 있으면 plain Text content를 같은 요청에서 쓴다.
- Text create 요청에서는 encrypted payload를 받지 않는다. Encrypted Text는 Text API `PUT /text/{node_id}`로 저장한다.
- File node create는 REST node create에서 허용하지 않는다. File은 REST File upload endpoint로 생성한다.
- 같은 parent 안 live name은 unique다.

## Update, move, delete

```ts
type UpdateNodeBody = {
  name?: string
  sort_order?: number
}

type MoveNodeBody = {
  new_parent_id: string
  new_name?: string
  expected_parent_id?: string
}
```

- `PATCH /nodes/{node_id}`는 같은 parent 안에서 rename 또는 reorder한다.
- Root node는 rename/move/delete할 수 없다.
- `POST /nodes/{node_id}/move`는 같은 Space 안에서만 parent/name을 변경한다.
- Folder 삭제는 `recursive=true`가 필요하다.
- 삭제는 soft delete이며, purge job이 retention 이후 hard delete할 수 있다.

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
