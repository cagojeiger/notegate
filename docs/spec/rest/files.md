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
GET  /api/v1/spaces/{space_id}/files/{node_id}/preview-url
GET  /api/v1/spaces/{space_id}/files/{node_id}/pdf-preview-url
GET  /api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url
POST /api/v1/spaces/{space_id}/file-previews:batchResolve
```

공통 schema는 `../schemas.md`를 따른다.

```ts
GET  /files/{node_id}   -> { node: RestNode }
GET  /files/{node_id}/content -> 302 presigned GET redirect
GET  /files/{node_id}/preview-url -> { url, media_type, expires_at }
GET  /files/{node_id}/pdf-preview-url -> { url, media_type, expires_at }
GET  /files/{node_id}/audio-preview-url -> { url, media_type, expires_at }
POST /file-previews:batchResolve -> { results: BatchFilePreviewResult[] }
```

## Upload

Permission: `write`.

1. `POST /file-uploads`에 `parent_node_id`, `name`, `byte_len`, `media_type`과 선택 encryption metadata를 보낸다.
2. `transfer.mode=single`이면 `transfer.url`에 `transfer.headers`를 적용해 전체 bytes를 PUT한다.
3. `transfer.mode=multipart`이면 `/parts`에 제한된 batch의 part number를 보내 URL을 발급받는다. 각 응답의 `content_length`만큼 원본을 잘라 제한된 동시성으로 PUT하고 응답 `ETag`를 기록한다. 실패한 part만 새 URL로 재시도한다. Batch와 동시성 상한은 [`performance-limits.md`](../performance-limits.md)의 Object upload 상한을 따른다.
4. `/complete`를 호출한다. Multipart는 모든 `{ part_number, etag }`를 `completed_parts`로 보낸다. Browser 녹음 기능이 File Node 생성 시 기록하는 metadata는 `node_metadata` object로 보내며, 공통 Node metadata 제한을 통과한 뒤 File Node 연결과 같은 DB transaction에서 한 번 저장된다.
5. 서버는 S3 `HEAD`로 실물 크기와 quota를 검증한 뒤 File Node를 생성한다. 암호화하지 않은 파일은 object 앞부분을 범위 조회해 실제 media type도 기록한다. 감지 실패는 완료를 막지 않는다.

### 전송 무결성

- Single PUT URL은 `If-None-Match: *`와 요청의 `Content-Length`를 서명한다. 같은 URL로 object를 덮어쓰거나 선언한 `byte_len`과 다른 크기를 올릴 수 없다.
- 브라우저가 직접 설정할 수 없는 `Content-Length`는 응답 header 목록에서 제외하고 user agent가 body 길이로 생성한다.
- Single PUT과 multipart part URL의 유효 시간은 [`performance-limits.md`](../performance-limits.md)의 Object upload 상한을 따른다.
- 브라우저와 저장소 사이의 직접 PUT 진행률은 NoteGate가 관찰하지 않는다.

### 상한과 생명주기

File size, upload mode, multipart geometry, pending handle 수와 만료 시간은 [`performance-limits.md`](../performance-limits.md)의 Object upload 상한을 따른다.

- 완료 전 upload는 File Node가 아니다.
- begin, part URL 재발급과 유효한 multipart 완료 요청은 activity 시각을 갱신한다.
- begin은 live File bytes와 진행 중인 선언 bytes를 함께 quota에 반영한다.
- Upload handle의 write-lock 예약과 완료 규칙은 [`files-commands.md`](../files-commands.md#write-lock)를 따른다.
- 취소는 `DELETE /file-uploads/{upload_id}`로 요청한다. 응답은 물리 삭제 완료가 아니라 cleanup 상태 등록을 뜻하며 provider 삭제는 object storage cleanup reconciliation이 재시도한다.

실행 중 저장소가 실패하면 File operation은 `503 object_storage_unavailable`을 반환한다. Provider, CORS와 runtime 설정은 [개발 가이드](../../development.md#object-storage)를 따른다.

## Metadata/stat

Permission: `read`.

`GET /files/{node_id}`는 file node의 metadata와 file stats를 반환한다. Browser 녹음 upload 완료 시 선택 `node_metadata`로 초기값을 원자적으로 저장할 수 있으며 생성 이후에는 외부 API에서 수정할 수 없다. 완료 재시도는 처음 연결된 File Node와 metadata를 그대로 반환한다.

`media_type`은 client 선언값이고 `detected_media_type`은 provider object에서 감지한 값이다.

- `preview_available`: image preview 가능 여부. PDF에서는 `false`다.
- `file_preview_kind`: preview 종류. PDF는 `"pdf"`다.
- `file_media_kind`: 표시용 `image | pdf | audio | other` 분류이며 preview URL 계약과 분리된다.
- Tree/Recent compact 요약은 image/PDF preview field를 포함하고, audio일 때만 `file_media_kind: "audio"`를 포함한다.

Inline preview는 client 선언값이 아니라 감지 결과와 안전한 container pair로 결정한다.

- MP4/WebM이 `video/*`로 감지되어도 선언값이 각각 `audio/mp4` 또는 `audio/webm`이면 audio로 분류한다.
- `detected_media_type`이 없으면 첫 preview URL 요청에서 감지한다.
- 감지값은 process-local write-behind로 저장하므로 같은 요청 직후의 metadata 조회에는 반영되지 않을 수 있다.

## Download

Permission: `read`.

`GET /files/{node_id}/content`는 S3 호환 object의 presigned GET URL로 `302` redirect한다.

- `encryption_mode=none`: 원본 bytes
- `encryption_mode=client`: 클라이언트 암호문 bytes

- 응답은 `Location`만 노출한다.
- URL은 한 object의 GET으로 제한되고 S3 자격증명을 포함하지 않는다. Lifetime은 [`performance-limits.md`](../performance-limits.md)의 REST/browser presigned URL 상한을 따른다.
- Presigned GET은 `original_filename` 유무와 관계없이 `Content-Disposition: attachment`를 서명해 저장소 origin의 inline rendering을 막는다.

## Image preview

Permission: `read`.

`GET /files/{node_id}/preview-url`은 다음 조건을 만족하는 파일에 임시 presigned GET URL을 반환한다.

- 10 MiB 이하
- 실제 bytes가 PNG, JPEG, WebP, AVIF 또는 GIF
- client encryption을 사용하지 않음

URL은 감지된 `Content-Type`과 `Content-Disposition: inline`을 서명한다. 응답은 `Cache-Control: private, no-store`다.

SVG, PDF, HTML, 알 수 없는 형식과 10 MiB 초과 File은 image preview 대상이 아니며 `404`를 반환한다. 원본 download는 형식과 무관하게 `/content` endpoint로 가능하다. Preview URL에는 NoteGate credential이 포함되지 않는다.

Markdown 본문의 여러 image path는 `POST /file-previews:batchResolve`로 조회한다.

- 요청: 중복 없는 정규화 path 1~64개, UTF-8 합계 16 KiB 이하
- 응답: 요청 순서를 유지하며 각 결과는 `ready`, `not_found`, `unsupported`, `error` 중 하나
- 권한/DB: Space read 권한을 한 번 확인하고 path와 File metadata를 고정된 수의 query로 조회
- URL 생성: 최대 4개씩 처리
- 감지값 저장: 단일 preview와 같은 process-local write-behind batch 사용
- Cache: `private, no-store`

일부 object storage 실패는 전체 batch를 실패시키지 않는다.

## PDF preview

Permission: `read`.

`GET /files/{node_id}/pdf-preview-url`은 10 MiB 이하이고 실제 bytes가 `application/pdf`인 파일에 임시 presigned GET URL을 반환한다. URL은 감지된 `Content-Type`과 `Content-Disposition: inline`을 서명하며 응답은 `Cache-Control: private, no-store`다.

PDF preview는 file detail 전용이다. Markdown image batch endpoint는 PDF를 `unsupported`로 유지한다.

## Audio preview

Permission: `read`.

`GET /files/{node_id}/audio-preview-url`은 실제 bytes가 `audio/*`이거나, 아래의 선언값과 일치하는 WebM/MP4 녹음 container인 파일에 임시 presigned GET URL을 반환한다.

- 감지값이 `video/webm` 또는 `video/mp4`인 브라우저 녹음은 선언값이 각각 `audio/webm` 또는 `audio/mp4`일 때만 대응하는 audio `Content-Type`으로 정규화한다.
- 선언값과 감지값이 맞지 않는 container, HTML, 알 수 없는 형식과 client-encrypted file은 `404`를 반환한다.

녹음 파일에는 image/PDF의 10 MiB preview 상한을 적용하지 않는다.

- URL은 `Content-Disposition: inline`, `Cache-Control: private, no-store, max-age=0`과 검증된 `Content-Type`을 서명한다.
- Provider의 byte range 요청을 지원한다.
- Browser `<audio>`는 필요한 구간을 object storage에서 `206 Partial Content`로 직접 읽는다.
