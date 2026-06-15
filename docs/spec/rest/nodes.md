# REST Nodes

Node API는 Space tree의 node 속성을 다룬다. Content body는 Text/File category에서 다룬다.

```http
GET    /api/v1/spaces/{space_id}/paths/resolve?path=/notes/state.json
GET    /api/v1/spaces/{space_id}/nodes?sort=updated_at_desc&kind=text&limit=50&cursor=...
GET    /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/children?limit=100&cursor=...
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
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
GET /nodes                      -> { nodes: RestNode[], page: Page }
GET /nodes/{node_id}            -> RestNode
GET /nodes/{node_id}/children   -> { parent: NodeRef, children: RestNode[], page: Page }
GET /nodes/{node_id}/reveal     -> { ancestors: RestNode[], target: RestNode }
POST /nodes                     -> RestNode
PATCH /nodes/{node_id}          -> RestNode
PUT/PATCH /nodes/{id}/metadata  -> RestNode
POST /nodes/{node_id}/move      -> RestNode
DELETE /nodes/{node_id}         -> 204 No Content
```

`RestNode`는 UI용 resource shape이므로 metadata, attribution, text/file summary를 포함한다. Text/File content body는 포함하지 않는다.

## List nodes

`GET /nodes`는 tree children API가 아니라 Space 전체 node를 정렬/필터해 반환하는 목록 API다. UI의 최근 수정 목록 같은 list view에서 사용한다.

```ts
type ListNodesQuery = {
  kind?: "folder" | "text" | "file"
  sort?: "updated_at_desc" | "name_asc" // default: updated_at_desc
  limit?: number
  cursor?: string
}
```

Rules:

- 반환 body는 `{ nodes: RestNode[], page: Page }`다.
- `sort=updated_at_desc`는 최근 수정 목록의 기본 정렬이다.
- `sort=name_asc`는 Space 전체 basename 정렬이다. Tree 구조 정렬은 children API가 담당한다.
- `kind`가 없으면 folder/text/file을 모두 반환한다.
- Space root node는 목록에서 제외한다. Root는 Space detail의 `root_node_id` 또는 path resolve로 접근한다.
- Cursor는 opaque string이다. Client는 해석하지 않고 같은 `sort`/`kind` query와 함께 전달한다.
- Cursor는 생성 당시 `sort`/`kind`에 묶인다. 다른 `sort` 또는 `kind`와 함께 재사용하면 `400 invalid_input`이다.
- Content body는 반환하지 않는다.

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

## Reveal node

`GET /nodes/{node_id}/reveal`은 lazy tree UI에서 선택된 node까지 부모 folder들을 펼치기 위한 API다.

```ts
type RevealResponse = {
  ancestors: RestNode[] // root부터 target parent까지
  target: RestNode
}
```

Rules:

- `ancestors`는 root node를 포함하고 target node는 포함하지 않는다.
- `target`은 요청한 live node다.
- 삭제된 node나 접근 권한이 없는 Space의 node는 반환하지 않는다.
- FE는 `ancestors` 순서대로 folder를 expand하고 필요한 children page를 로드한 뒤 `target`을 선택한다.

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
- Markdown Text의 YAML frontmatter는 문서 content이며 Node `metadata`가 아니다. REST metadata API는 frontmatter를 읽거나 쓰지 않는다.

Update rules:

- `GET /metadata`는 `{ "metadata": {...} }`를 반환한다.
- `PUT /metadata`는 `{ "metadata": {...} }`로 metadata 전체를 교체한다.
- `PATCH /metadata`는 `{ "patch": {...} }`로 JSON Merge Patch 방식 부분 수정을 수행한다.
- PATCH에서 `null` 값은 해당 key 삭제를 의미한다.
