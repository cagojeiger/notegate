# REST Files

File은 binary/object content node다. Text read/patch/grep 대상이 아니다.

## Endpoints

```text
POST /api/v1/spaces/{space_id}/files
GET  /api/v1/spaces/{space_id}/files/{node_id}
GET  /api/v1/spaces/{space_id}/files/{node_id}/content
```

공통 schema는 `../schemas.md`를 따른다.

```ts
POST /files             -> { node: RestNode }
GET  /files/{node_id}   -> { node: RestNode }
GET  /files/{node_id}/content -> stored bytes
```

## Upload

Permission: `write`.

현재 REST File API는 inline small file만 허용한다.

```text
multipart/form-data
- parent_node_id: UUID
- name: file node name
- file: bytes
- media_type: optional; 기본 application/octet-stream
- original_filename: optional; 없으면 multipart filename 사용
- encryption_mode: optional none|client; 기본 none
- encryption_metadata: encryption_mode=client일 때 JSON object
```

제한:

```text
stored bytes <= 262144  # 256 KiB
```

`encryption_mode=client`이면 `file` part에는 클라이언트가 암호화한 bytes를 보낸다. 서버는 복호화하지 않는다. `byte_len`과 `content_sha256`은 저장된 bytes 기준이다.

256 KiB 초과 file은 `file_max_bytes` 안에 있어도 현재 API에서 거부한다.

## Metadata/stat

Permission: `read`.

`GET /files/{node_id}`는 file node의 metadata와 file stats를 반환한다. Node metadata는 공통 metadata API로 수정한다.

## Download

Permission: `read`.

`GET /files/{node_id}/content`는 저장된 bytes를 그대로 반환한다.

- `encryption_mode=none`: 원본 bytes
- `encryption_mode=client`: 클라이언트 암호문 bytes

응답 header:

```text
Content-Type: stored media_type
X-Content-Sha256: stored bytes sha256
X-Encryption-Mode: none|client
Content-Disposition: attachment; filename="..."  # original_filename이 있으면
```
