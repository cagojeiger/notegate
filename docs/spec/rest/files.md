# REST Files

File은 binary/object content node다. Text content operation이나 search op=grep 대상이 아니다.

## Endpoints

```text
POST /api/v1/spaces/{space_id}/file-uploads
POST /api/v1/spaces/{space_id}/file-uploads/{upload_id}/parts
POST /api/v1/spaces/{space_id}/file-uploads/{upload_id}/complete
DELETE /api/v1/spaces/{space_id}/file-uploads/{upload_id}
GET  /api/v1/spaces/{space_id}/files/{node_id}
GET  /api/v1/spaces/{space_id}/files/{node_id}/content
```

공통 schema는 `../schemas.md`를 따른다.

```ts
GET  /files/{node_id}   -> { node: RestNode }
GET  /files/{node_id}/content -> 302 presigned GET redirect
```

## Upload

Permission: `write`.

1. `POST /file-uploads`에 `parent_node_id`, `name`, `byte_len`, `media_type`과 선택 metadata를 보낸다.
2. `transfer.mode=single`이면 `transfer.url`에 `transfer.headers`를 적용해 전체 bytes를 PUT한다.
3. `transfer.mode=multipart`이면 `/parts`에 part number를 최대 16개씩 보내 URL을 발급받는다. 각 응답의 `content_length`만큼 원본을 잘라 최대 4개를 병렬 PUT하고 응답 `ETag`를 기록한다. 실패한 part만 새 URL로 재시도한다.
4. `/complete`를 호출한다. Multipart는 모든 `{ part_number, etag }`를 `completed_parts`로 보낸다. S3 `HEAD`로 실물 크기를 검증하고 Notegate quota 검사를 통과하면 File node가 생성된다.

Single PUT은 `If-None-Match: *`와 요청의 `Content-Length`를 서명하므로 같은 URL로 object를 덮어쓰거나 선언한 `byte_len`과 다른 크기를 업로드할 수 없다. 브라우저가 직접 설정할 수 없는 `Content-Length`는 응답 header 목록에서 제외하며 user agent가 body 길이로 자동 생성한다. Single과 multipart part presigned URL은 15분 동안 유효하다.

REST/browser 상한은 10737418240 bytes다. 104857600 bytes 이하는 single PUT, 초과 파일은 67108864-byte part의 multipart를 사용한다. 전체 File hard max와 MCP multipart 상한은 107374182400 bytes다. S3 설정은 API 시작에 필수다. 실행 중 저장소가 일시적으로 실패하면 file operation은 `503 object_storage_unavailable`을 반환한다. 완료 전 upload는 File node가 아니며, 2시간 동안 Notegate API 활동이 없으면 정리 대상이 된다. begin과 part URL 재발급, 유효한 multipart 완료 요청은 활동 시각을 갱신한다. 브라우저와 저장소 사이의 직접 PUT 진행률은 Notegate가 관찰하지 않는다. begin 시 live File bytes와 진행 중 선언 bytes를 함께 검사하고, 물리 삭제 전인 `uploading`과 `expire_pending` upload를 account당 16개로 제한한다.

사용자가 취소하면 `DELETE /file-uploads/{upload_id}`로 정리를 요청한다. Provider 삭제는 cleanup worker가 재시도하므로 응답은 물리 삭제 완료가 아니라 cleanup queue 등록을 뜻한다.

브라우저가 `PUBLIC_ENDPOINT`로 직접 PUT/GET할 수 있도록 S3 provider의 CORS는 Notegate origin과 `PUT`, `GET`, `Content-Type`, `If-None-Match` header를 허용하고 `ETag` 응답 header를 노출해야 한다. Multipart 완료에는 각 part의 `ETag`가 필요하다. 로컬 오픈소스 MinIO Compose는 버킷별 CORS 대신 `MINIO_API_CORS_ALLOW_ORIGIN`으로 서버 전역 origin을 설정한다. `ENDPOINT`는 서버 내부 주소이고 `PUBLIC_ENDPOINT`는 브라우저가 접근하고 서명에 사용하는 주소다.

Bucket은 운영자가 미리 생성한다. Notegate는 `CreateBucket`을 호출하지 않으며 설정된 기존 bucket만 사용한다. Bucket이 없거나 접근 권한이 없으면 object storage 요청은 실패한다.

필수 runtime 설정:

```text
NOTEGATE_S3__ENDPOINT
NOTEGATE_S3__REGION
NOTEGATE_S3__BUCKET
NOTEGATE_S3__ACCESS_KEY
NOTEGATE_S3__SECRET_KEY
```

브라우저가 내부 endpoint에 접근할 수 없으면 `NOTEGATE_S3__PUBLIC_ENDPOINT`도 설정한다. `NOTEGATE_S3__FORCE_PATH_STYLE`은 기본 `true`이며 provider에 맞게 변경한다. Access key와 secret key는 secret manager에서 주입한다.

## Metadata/stat

Permission: `read`.

`GET /files/{node_id}`는 file node의 metadata와 file stats를 반환한다. Node metadata는 공통 metadata API로 수정한다.

## Download

Permission: `read`.

`GET /files/{node_id}/content`는 S3 호환 object의 presigned GET URL로 `302` redirect한다.

- `encryption_mode=none`: 원본 bytes
- `encryption_mode=client`: 클라이언트 암호문 bytes

응답은 `Location`만 노출하며 S3 자격증명이나 물리 object key를 포함하지 않는다. Presigned GET은 `original_filename` 유무와 무관하게 항상 `Content-Disposition: attachment`를 서명에 포함해, client가 선언한 `media_type`이 저장소 origin에서 inline으로 렌더링되지 않도록 한다.
