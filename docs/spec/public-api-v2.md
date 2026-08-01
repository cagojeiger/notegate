# V2 공개 API

V2는 외부 확장이 사용하는 Agent API key 전용 resource API다. 브라우저 UI의 V1 helper와 MCP의 `space:/path` command payload를 공개 계약으로 재사용하지 않는다.

## 인증과 권한

```http
Authorization: Bearer ngk_v2_...
```

- Agent 소유 `ngk_v2_` key만 허용한다.
- Browser session cookie, OAuth JWT, User 소유 API key, 이전 prefix의 key는 인증되지 않는다.
- Agent는 연결된 Space만 볼 수 있고 connection의 `read` 또는 `write` 권한을 그대로 적용받는다.
- Node write lock, quota, 이름과 경로 검증, audit/change event 기록은 기존 service layer가 집행한다.
- 모든 V2 응답은 `Cache-Control: private, no-store`로 전달한다.

### 빠른 시작

```bash
export NOTEGATE_BASE_URL='https://<notegate-host>'
export NOTEGATE_API_KEY='ngk_v2_...'

curl --fail-with-body --silent --show-error \
  -H "Authorization: Bearer ${NOTEGATE_API_KEY}" \
  "${NOTEGATE_BASE_URL}/api/v2/me"
```

호출 가능한 Space와 권한은 Space 목록에서 확인한다.

```bash
curl --fail-with-body --silent --show-error \
  -H "Authorization: Bearer ${NOTEGATE_API_KEY}" \
  "${NOTEGATE_BASE_URL}/api/v2/spaces?limit=50"
```

Agent가 보내는 `account_id`나 `user_id`는 없다. 서버가 API key에서 caller를 결정하고 연결된 Space와 `read`/`write` 권한을 적용한다.

### 공통 요청 규칙

- 요청과 응답 본문은 별도 표기가 없으면 `application/json`이다.
- `{space_id}`, `{node_id}`, `{upload_id}`는 UUID다.
- Space 내부 path는 `/`로 시작하는 절대 경로다. 응답 path는 정규화된 canonical path다.
- `cursor`는 opaque 값이다. 같은 endpoint와 같은 filter에 `next_cursor`를 그대로 전달한다.
- `expected_sha256`과 `expected_parent_id`는 선택적인 낙관적 동시성 guard다. 불일치하면 `409 conflict`이며 최신 상태를 다시 읽어야 한다.

목록 상한은 다음과 같다. 서버는 생략된 값을 기본값으로 바꾸고 최대값보다 큰 값은 최대값으로 제한한다.

| 작업 | 기본 `limit` | 최대 `limit` |
|---|---:|---:|
| Space 목록 | 50 | 100 |
| 직계 자식, Tree | 100 | 200 |
| Find | 50 | 100 |
| Grep | 20 | 100 |

페이지 응답에서 `has_more=true`이면 `next_cursor`가 존재한다. 다음 호출은 cursor 이외의 scope, query, match mode, include/exclude 값을 바꾸지 않는다.

## 공개 경로

### Identity와 Space

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/api/v2/me` | 현재 Agent caller 확인 |
| `GET` | `/api/v2/spaces` | 연결된 Space 목록 |
| `GET` | `/api/v2/spaces/{space_id}` | 연결된 Space 조회 |

### Node와 Tree

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/api/v2/spaces/{space_id}/paths/resolve?path=/...` | 절대 경로를 Node로 해석 |
| `GET` | `/api/v2/spaces/{space_id}/tree` | 제한된 깊이의 subtree 조회 |
| `POST` | `/api/v2/spaces/{space_id}/nodes` | Folder 또는 Text 생성 |
| `GET` | `/api/v2/spaces/{space_id}/nodes/{node_id}` | Node 상세와 유효 write lock 조회 |
| `GET` | `/api/v2/spaces/{space_id}/nodes/{node_id}/children` | 직계 자식 페이지 조회 |
| `POST` | `/api/v2/spaces/{space_id}/nodes/{node_id}/move` | 같은 Space 안에서 이동 또는 이름 변경 |
| `POST` | `/api/v2/spaces/{space_id}/nodes/{node_id}/copy` | 같은 Space 안에서 복사 |
| `DELETE` | `/api/v2/spaces/{space_id}/nodes/{node_id}` | Node soft delete |

Node 생성에서 File은 허용하지 않는다. File은 upload lifecycle을 통해서만 생성한다. 목록 응답의 `cursor`는 opaque 값이며 클라이언트가 해석하거나 생성하지 않는다.

Tree의 `depth` 기본값은 2, 최대값은 7이다. 폴더나 Text 생성 본문은 다음과 같다.

```json
{
  "parent_id": "11111111-1111-1111-1111-111111111111",
  "name": "README.md",
  "kind": "text",
  "content": "# Project notes\n"
}
```

### Text

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/api/v2/spaces/{space_id}/text/{node_id}` | 범위 또는 조건부 평문 읽기 |
| `PUT` | `/api/v2/spaces/{space_id}/text/{node_id}` | 전체 평문 교체 |
| `PATCH` | `/api/v2/spaces/{space_id}/text/{node_id}` | 정확한 문자열 치환 |
| `POST` | `/api/v2/spaces/{space_id}/text/{node_id}/append` | 평문 뒤에 추가 |
| `POST` | `/api/v2/spaces/{space_id}/text/{node_id}/edit` | 줄 단위 편집 |

변경 요청은 선택적으로 `expected_sha256`을 받아 낙관적 동시성 제어를 수행한다. Client-encrypted Text는 V2로 읽거나 수정할 수 없다. 서버 관리 at-rest 암호화는 service layer에서 투명하게 처리한다.

Text 읽기는 기본 200줄/65,536 bytes, 최대 5,000줄/1,048,576 bytes를 반환한다. `truncated=true`이면 `next_start_line`을 다음 요청의 `start_line`으로 사용한다. `content_sha256`은 현재 page가 아니라 전체 Text의 hash다.

```bash
curl --fail-with-body --silent --show-error \
  -H "Authorization: Bearer ${NOTEGATE_API_KEY}" \
  "${NOTEGATE_BASE_URL}/api/v2/spaces/${SPACE_ID}/text/${NODE_ID}?start_line=1&max_lines=200"
```

`if_none_match_sha256`이 현재 hash와 같아도 HTTP `304`가 아니라 `200`을 반환한다. 이때 `text.unchanged=true`, `text.content_returned=false`이며 본문 content는 포함하지 않는다.

전체 교체 예시:

```json
{
  "content": "# Updated document\n",
  "expected_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}
```

문자열 `PATCH`의 `mode`는 `unique`(기본), `first`, `all`이다. 줄 편집의 `op`와 필요한 필드는 다음과 같다.

| `op` | 필수 필드 |
|---|---|
| `insert_before_line` | `line`, `content` |
| `insert_after_line` | `line`, `content` |
| `replace_lines` | `start_line`, `end_line`, `content` |
| `delete_lines` | `start_line`, `end_line` |

줄 번호는 1부터 시작하고 `start_line`과 `end_line`은 모두 포함한다.

### Search

| Method | Path | 설명 |
|---|---|---|
| `POST` | `/api/v2/spaces/{space_id}/search/find` | 이름 또는 경로 검색 |
| `POST` | `/api/v2/spaces/{space_id}/search/grep` | 평문 본문 검색 |

Search는 include/exclude glob, pagination, process-wide admission limit을 사용한다. 용량이 가득 차면 `429 search_busy`와 `Retry-After`를 반환한다.

| 작업 | `kind` | `match` | `lines` | 기본 path |
|---|---|---|---|---|
| Find | 선택: `folder`, `text`, `file` | `contains`(기본), `regex`, `glob` | 해당 없음 | `/` |
| Grep | 해당 없음 | `literal`(기본), `regex` | `none`(기본), `first`, `all` | `/` |

`include`와 `exclude`는 canonical relative path에 적용하는 glob 배열이다. 검색 query는 한 줄이며 최대 256자다. 패턴 배열은 각각 최대 32개, 패턴 하나는 최대 256자다.

```json
{
  "q": "TODO",
  "path": "/docs",
  "match": "literal",
  "lines": "first",
  "include": ["**/*.md"],
  "exclude": ["archive/**"],
  "limit": 20
}
```

### File transfer

| Method | Path | 설명 |
|---|---|---|
| `POST` | `/api/v2/spaces/{space_id}/file-uploads` | upload ledger 생성과 single/multipart 전송 정보 발급 |
| `POST` | `/api/v2/spaces/{space_id}/file-uploads/{upload_id}/parts` | multipart part URL 발급 |
| `POST` | `/api/v2/spaces/{space_id}/file-uploads/{upload_id}/complete` | provider upload 완료 확인 후 File Node 연결 |
| `DELETE` | `/api/v2/spaces/{space_id}/file-uploads/{upload_id}` | 미완료 upload 정리 예약 |
| `GET` | `/api/v2/spaces/{space_id}/files/{node_id}/download` | download URL 발급 |

File bytes는 NoteGate JSON을 통과하지 않고 S3 호환 presigned URL로 전송한다. URL 유효 시간은 5분이며 URL 자체를 bearer credential처럼 취급해 로그나 영구 저장소에 남기지 않는다. 시스템 파일 상한은 100 GiB이며 실제 허용량은 Space tier와 잔여 quota에 따라 더 작을 수 있다. 100 MiB 이하는 single PUT, 그보다 큰 파일은 multipart를 사용한다.

#### Single PUT

1. `POST /file-uploads`로 ledger를 만들고 `transfer.mode=single`을 확인한다.
2. `transfer.url`로 정확히 `byte_len` bytes를 `PUT`한다. `transfer.headers`의 모든 header를 그대로 포함하며 NoteGate `Authorization` header는 provider URL에 보내지 않는다.
3. provider PUT 성공 후 `POST /file-uploads/{upload_id}/complete`에 `{}`를 보낸다.
4. complete 응답의 File Node가 반환된 뒤에만 업로드가 NoteGate tree에 연결된 것으로 본다.

#### Multipart PUT

1. `POST /file-uploads` 응답에서 `transfer.mode=multipart`, `part_size`, `part_count`를 확인한다.
2. `/parts`에 1부터 시작하는 `part_numbers`를 보낸다. 한 요청은 최대 16개 part URL을 발급한다.
3. 각 URL에 응답의 `headers`와 정확한 `content_length`를 사용해 `PUT`한다. 권장 동시 업로드 상한은 응답의 `upload_concurrency_max`이며 현재 4다. 실패한 part만 새 URL을 발급받아 다시 전송할 수 있다.
4. 각 PUT 응답의 ETag와 part number를 보관한다.
5. 모든 part가 성공하면 `/complete`에 전체 part 목록을 보낸다.

```json
{
  "completed_parts": [
    {"part_number": 1, "etag": "\"provider-etag-1\""},
    {"part_number": 2, "etag": "\"provider-etag-2\""}
  ]
}
```

Client는 전송 성공 후 반드시 complete를 호출한다. 중단할 때는 `DELETE /file-uploads/{upload_id}`로 cleanup을 예약한다. Presigned URL 만료와 upload ledger 만료는 별개이며, complete가 성공하기 전에는 File Node가 생성된 것으로 취급하지 않는다.

### 오류 처리

오류 본문은 [`rest/errors.md`](rest/errors.md)의 공통 JSON shape을 사용한다. 클라이언트는 사람이 읽는 `message`가 아니라 안정적인 `error` 값으로 분기한다.

| HTTP | 주요 `error` | 의미 |
|---:|---|---|
| 400 | `invalid_input` | 잘못된 UUID, path, option 또는 body |
| 401 | `missing_token`, `invalid_token` | Agent API key가 없거나 유효하지 않음 |
| 403 | `forbidden` | 연결 권한 또는 write 권한 부족 |
| 404 | `not_found` | 보이지 않거나 존재하지 않는 resource |
| 405 | `method_not_allowed` | resource에서 지원하지 않는 HTTP method |
| 408 | `request_timeout` | 요청 처리 시간 상한 초과 |
| 409 | `conflict` | hash/parent 불일치, 중복 또는 상태 충돌 |
| 413 | `payload_too_large` | 요청 본문 크기 상한 초과 |
| 423 | `node_write_locked`, `subtree_write_locked` | 직접·상속 write lock에 의해 변경 차단 |
| 429 | `search_busy`, `rate_limited` | 검색 또는 HTTP 처리 용량 초과 |
| 500 | `internal_error` | 공개하지 않는 내부 처리 실패 |
| 503 | `object_storage_unavailable`, `usage_recalculation_in_progress` | 일시적인 의존성 또는 유지보수 상태 |

재시도 가능한 응답은 `Retry-After` header를 포함할 수 있다.

## 제외 범위

V2는 Agent가 연결된 Space 안에서 수행하는 resource 작업만 제공한다. 다음 User 또는 운영자 기능은 공개하지 않는다.

- Space 생성, 수정, 삭제, 정렬
- Agent, API key, connection 관리
- Node metadata JSON 조회와 수정
- 검색 포함 정책, Text 암호화, write lock 변경
- Browser preview helper
- Audit log와 file change sync
- 계정 삭제와 사용량 reconciliation

## 운영과 문서

Browser V1, Public V2, User MCP, Agent MCP V2는 독립된 in-process rate-limit bucket을 사용하고 전체 ingress hard limit을 공유한다. 이 제한은 tier quota가 아니라 process-wide 안전 상한이다. V2를 별도 프로세스로 분리하더라도 path와 DTO 계약은 유지하고 domain authorization과 invariant는 같은 service layer를 사용한다.

OpenAPI JSON은 `/openapi/v2.json`, Swagger UI는 `/swagger-ui/v2`에서 제공한다. 문서 경로는 browser session을 요구하고 미로그인 브라우저는 로그인 후 요청했던 문서 경로로 복귀한다. Swagger에서 API를 실행할 때는 별도의 Agent API key를 `Authorize`에 입력한다.
