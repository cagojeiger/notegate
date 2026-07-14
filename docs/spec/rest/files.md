# REST Files

File은 binary/object content node다. Text content operation이나 search op=grep 대상이 아니다.

## Endpoints

```text
POST /api/v1/spaces/{space_id}/files
POST /api/v1/spaces/{space_id}/file-uploads
POST /api/v1/spaces/{space_id}/file-uploads/{upload_id}/complete
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

### Inline

```text
multipart/form-data
- parent_node_id: UUID
- name: file node name
- file: bytes
- media_type: optional; 없으면 multipart file part content-type 사용, 그것도 없으면 application/octet-stream
- original_filename: optional; 없으면 multipart filename 사용
- encryption_mode: optional none|client; 기본 none
- encryption_metadata: encryption_mode=client일 때 JSON object
```

제한:

```text
stored bytes <= 262144  # 256 KiB
```

`encryption_mode=client`이면 `file` part에는 클라이언트가 암호화한 bytes를 보낸다. 서버는 복호화하지 않는다. `byte_len`과 `content_sha256`은 저장된 bytes 기준이다.

### S3 호환 object

1. `POST /file-uploads`에 `parent_node_id`, `name`, `byte_len`, `media_type`과 선택 metadata를 보낸다.
2. 응답의 `transfer.url`에 `transfer.headers`를 모두 적용해 bytes를 PUT한다. `If-None-Match: *`가 서명되므로 같은 URL로 object를 덮어쓸 수 없다. Presigned URL은 15분 동안 유효하다.
3. `/complete`를 호출한다. S3 `HEAD`로 실물 크기를 검증하고 Notegate quota 검사를 통과하면 File node가 생성된다.

현재 제품 상한은 104857600 bytes이며 single PUT만 지원한다. S3 저장소가 설정되지 않았거나 일시적으로 실패하면 `503 object_storage_unavailable`을 반환한다. 완료 전 upload는 File node가 아니며, 30분 동안 활동이 없으면 정리 대상이 된다. quota는 `/complete`에서만 부과되므로, 한 account가 완료하지 않은 upload를 무한히 스테이징하지 못하도록 동시 진행 upload를 16개로 제한한다. 초과하면 `POST /file-uploads`는 `409 conflict`를 반환한다.

브라우저가 `PUBLIC_ENDPOINT`로 직접 PUT/GET할 수 있도록 S3 provider의 CORS는 Notegate origin과 `PUT`, `GET`, `Content-Type`, `If-None-Match` header를 허용해야 한다. 로컬 오픈소스 MinIO Compose는 버킷별 CORS 대신 `MINIO_API_CORS_ALLOW_ORIGIN`으로 서버 전역 origin을 설정한다. `ENDPOINT`는 서버 내부 주소이고 `PUBLIC_ENDPOINT`는 브라우저가 접근하고 서명에 사용하는 주소다.

Bucket은 운영자가 미리 생성한다. Notegate는 `CreateBucket`을 호출하지 않으며 설정된 기존 bucket만 사용한다. Bucket이 없거나 접근 권한이 없으면 object storage 요청은 실패한다.

## Metadata/stat

Permission: `read`.

`GET /files/{node_id}`는 file node의 metadata와 file stats를 반환한다. Node metadata는 공통 metadata API로 수정한다.

## Download

Permission: `read`.

`GET /files/{node_id}/content`는 inline bytes를 반환하거나 S3 호환 object의 presigned GET URL로 `302` redirect한다.

- `encryption_mode=none`: 원본 bytes
- `encryption_mode=client`: 클라이언트 암호문 bytes

Inline 응답 header:

```text
Content-Type: stored media_type
X-Content-Sha256: stored bytes sha256
X-Encryption-Mode: none|client
Content-Disposition: attachment; filename="..."  # original_filename이 있으면
```

Object 응답은 `Location`만 노출하며 S3 자격증명이나 물리 object key를 포함하지 않는다. Presigned GET은 `original_filename` 유무와 무관하게 항상 `Content-Disposition: attachment`를 서명에 포함해, client가 선언한 `media_type`이 저장소 origin에서 inline으로 렌더링되지 않도록 한다.
