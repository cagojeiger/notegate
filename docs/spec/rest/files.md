# REST Files

File은 binary/object content node다. 직접 text read/patch/grep 대상이 아니다.

```http
POST /api/v1/spaces/{space_id}/files
GET  /api/v1/spaces/{space_id}/files/{node_id}
GET  /api/v1/spaces/{space_id}/files/{node_id}/download
```

Rules:

- `node_id`는 `nodes.kind='file'`이어야 한다.
- 256 KiB 이하 file은 PostgreSQL inline 저장이 가능하다.
- 더 큰 file은 object storage 구현 시 object key로 저장한다.
- Response metadata는 `media_type`, `byte_len`, `content_sha256`, `storage_kind`를 포함한다.
- File content는 필요 시 server-side encryption으로 저장할 수 있다.
