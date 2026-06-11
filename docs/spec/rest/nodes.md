# REST Nodes

Node API는 tree metadata를 다룬다. Content는 Text/File category에서 다룬다.

```http
GET    /api/v1/spaces/{space_id}/paths/resolve?path=/notes/state.json
GET    /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/children?limit=100&cursor=...
POST   /api/v1/spaces/{space_id}/nodes
PATCH  /api/v1/spaces/{space_id}/nodes/{node_id}
POST   /api/v1/spaces/{space_id}/nodes/{node_id}/move
DELETE /api/v1/spaces/{space_id}/nodes/{node_id}?recursive=true
```

Node kind:

```text
folder
text
file
```

Create rules:

- Folder create는 `nodes(kind='folder')`만 만든다.
- Text create는 `nodes(kind='text')`와 `text_objects`를 함께 만든다.
- Text create 요청에 `content`가 있으면 content를 같은 요청에서 쓴다.
- File node create는 REST node create에서 허용하지 않는다.
- 같은 parent 안 live name은 unique다.
