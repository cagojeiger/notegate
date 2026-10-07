# Lifecycle

이 문서는 user, agent, space, connection, node, text, file, API key의 생성/삭제 side effect 정본이다.

## User

### Local user 최초 생성

```text
accounts(kind='user')
users
```

- user 생성은 space, agent, API key를 자동 생성하지 않는다.
- users.tier는 `NOTEGATE_DEFAULT_USER_TIER` 값으로 설정한다.
- 첫 space는 user가 명시적으로 생성한다.
- 재로그인은 PII ciphertext/hash만 갱신하고 space/agent/key를 만들지 않는다.

### User 삭제

User 삭제는 live owned space가 없을 때만 허용한다. Space는 사용자가 먼저 명시적으로 삭제해야 한다.

t=0:

```text
accounts.is_active=false
accounts.deleted_at/deleted_by_account_id 설정
owned agents deactivate
owned agent API keys revoke
owned browser sessions revoke
owned agent connections disconnect
```

purge 시점:

```text
users PII ciphertext/hash 제거
provider_sub_hash tombstone 해제
attribution 보존용 account/user shell 유지
```

## Agent

### Agent 생성

User caller만 agent를 생성한다.

```text
accounts(kind='agent')
agents(owner_user_id=user)
```

- API key는 자동 생성하지 않는다.
- Space connection은 자동 생성하지 않는다.
- owner user당 active agent 한도를 넘을 수 없다.

### Agent 삭제

Agent 삭제는 deactivate다.

```text
accounts(kind='agent').is_active=false
agent API keys revoke
space_agent_connections disconnect
```

`agents` row는 attribution 보존을 위해 일반 product action에서 hard delete하지 않는다.

## Space

### Space 생성

User caller만 space를 생성한다.

```text
spaces(owner_user_id=user, sort_order=0)
root node '/'
space_usage(live_node_count=1, live_text_bytes=0, live_file_bytes=0)
```

- Space는 owner user의 live space 한도를 넘을 수 없다.
- Root node는 생성 transaction의 일부다.
- Agent는 space를 생성할 수 없다.

### Space update/delete

Owner user만 space 이름과 sort_order 변경, 삭제를 수행한다.

삭제는 soft delete다.

```text
spaces.deleted_at=now()
spaces.deleted_by_user_id=caller
spaces.purge_after=now()+retention
```

- 내부 nodes/text/file/connection은 즉시 hard delete하지 않는다.
- Space와 현재 문서·파일은 휴지통에서 30일 보관한다. 만료 또는 사용자 영구 삭제 요청 이후 purge가 S3 object를 `delete_pending`으로 전환하고 정리 worker가 삭제를 재시도한다.
- 연결 row는 즉시 disconnect하지 않는다. 삭제된 space는 live 조회와 권한 확인에서 제외되어 agent 접근이 차단된다.
- `space_usage`는 purge까지 유지하지만 Usage 조회와 reconciliation 대상에서는 제외한다.
- Live 조회는 deleted space를 제외한다.
- `purge_after` 이후 background purge가 하위 자원을 batch로 정리하고 빈 Space를 제거한다.

## Agent connection

Owner user만 agent를 space에 연결/해제한다.

```text
space_agent_connections
  permission = read | write
```

- 연결 대상 agent는 같은 owner user의 active agent여야 한다.
- 연결 대상 space는 caller가 소유한 live space여야 한다.
- `write`는 `read`를 포함한다.
- Connection 변경은 account, agent, space, API key를 만들지 않는다.

## Text and File nodes

### Folder 생성

```text
nodes(kind='folder')
```

### Text 생성/쓰기

```text
nodes(kind='text')
text_objects
```

- plain Text content는 UTF-8이다.
- plain Text는 `byte_len`, `line_count`, `content_sha256`을 plaintext 기준으로 저장한다. `media_type`은 Text object 속성으로 저장한다.
- encrypted Text는 client-side encrypted payload를 저장하고 `line_count=0`을 사용한다.
- 서버 관리 Text 암호화 설정을 변경하면 기존 plain Text 본문을 같은 transaction에서 즉시 암호화하거나 복호화한다.
- 검색 포함 여부와 서버 관리 Text 암호화 설정은 Space owner User만 별도 API로 변경한다. Agent의 write 권한은 이 정책 관리를 포함하지 않는다.
- REST read/write는 plain Text와 client-side encrypted payload를 모두 다룬다. REST patch는 plain Text만 대상으로 한다. MCP Text content operation과 `search op=grep`은 plain Text만 대상으로 하며 서버 관리 암호화는 서버에서 투명하게 복호화한다.

### File

```text
nodes(kind='file')
file_objects
object_storage_objects
```

- File은 binary/object content다.
- File size와 single/multipart 기준은 [`performance-limits.md`](./performance-limits.md)의 Object upload 상한을 따른다. REST와 MCP 모두 `HEAD` 크기 검증 뒤 File node를 연결하고, 암호화하지 않은 파일은 실제 media type을 감지한다.
- Object download는 S3 호환 presigned GET URL로 redirect한다.
- 10 MiB 이하 PNG, JPEG, WebP, AVIF, GIF는 image preview URL을, 10 MiB 이하 PDF와 검증된 DOCX package는 file detail 전용 preview URL을 발급할 수 있다. 실제 bytes에서 media type과 DOCX package 구조를 검증하며 SVG, client-encrypted file과 10 MiB 초과 file은 preview 대상이 아니다.
- 완료되지 않은 upload와 soft-delete된 File의 물리 삭제는 `object_storage_objects` 원장과 정리 worker가 재시도한다. 완료된 정리 이력은 cluster-singleton purge가 90일 뒤 삭제한다.
- Provider multipart 생성과 DB 원장 기록은 하나의 transaction이 아니므로, S3 provider에도 incomplete multipart 자동 abort 정책을 설정한다. 로컬 MinIO는 server 기본 stale multipart cleanup을 2차 안전망으로 사용한다.
- MCP `file_upload`/`file_download`는 임시 presigned URL만 제공하고 bytes는 MCP payload를 통과하지 않는다. URL lifetime은 `performance-limits.md`, Node metadata는 REST metadata API를 따른다.
- File은 `read op=read`, `write op=patch/edit`, `search op=grep` 대상이 아니다.

### Node 삭제

Folder/Text/File 삭제는 soft delete다.

```text
nodes.deleted_at=now()
nodes.deleted_by_account_id=caller
nodes.purge_after=now()+retention
```

Folder recursive delete는 subtree node를 같은 transaction에서 soft delete한다.

삭제된 subtree의 문서·S3 object는 30일 보관한다. `deletion_target_node_id`로 이번 삭제 묶음만 복원하며, 먼저 별도로 삭제했던 자식은 복원하지 않는다. 보관 기간 만료 또는 영구 삭제 요청 이후 purge가 S3 삭제를 예약한다.

### 휴지통

- 삭제마다 새 `deletion_operation_id`를 저장하고 삭제 event의 `operation_id`와 연결한다. 복원/영구 삭제 요청은 새 작업 ID와 원래 삭제 참조를 기록한다. 기존 행은 NULL을 유지한다. 복원 범위는 기존 `deletion_target_node_id` 기준을 유지하며 로그 보존 여부에 의존하지 않는다.
- Browser owner user 전용: `GET /api/v1/me/trash`는 삭제 시각/id 순으로 cursor pagination한다. 삭제된 Space 내부 항목은 Space 복원 이후 별도로 조회한다.
- `POST /api/v1/me/trash/spaces/{space_id}/restore` 또는 `/nodes/{node_id}/restore`로 원래 위치에 복원한다. 이름 충돌, 삭제된 부모, write lock, 현재 tier/usage/path 제한은 복원을 거절한다.
- Space 복원은 기존 agent 연결을 해제한다. 외부 접근은 owner가 다시 연결해야 한다. 기존 node external-access 정책은 유지한다.
- 동일 경로의 `DELETE`는 `202 deletion_requested`를 반환하고 `purge_requested_at`을 기록해 시간과 무관하게 즉시 복원을 금지한다. 정리는 기존 purge/object-storage Reconciler가 비동기로 재시도한다. 저장소 실제 삭제 완료를 뜻하지 않는다.
- 복원과 purge는 같은 Space gate/row lock으로 직렬화한다. 복원 시각이 `purge_after` 이상이면 복원할 수 없다.
- 기존 삭제 건은 S3 bytes가 이미 제거됐을 수 있으므로 복원을 제공하지 않고 기존 만료 시각을 연장하지 않는다. 새 정책은 모든 이전 replica가 교체된 뒤 발생한 삭제부터 보장한다.
- 이미 발급된 S3 presigned URL은 원본 보존 중 만료 시각까지 동작할 수 있다. 새 URL 발급과 live content API는 soft delete 직후 차단한다.
- 현재 본문 복원과 과거 Text revision 보존 정책은 별개다. 휴지통 이동은 과거 버전의 TTL을 연장하지 않는다.
- 복원·영구 삭제 요청은 조회한 `deleted_at`과 `deletion_operation_id`를 query로 전달한다. 작업 ID가 없는 기존 행도 삭제 시각은 필수이며, 잠금 안에서 현재 삭제 건과 일치하지 않으면 409로 거절한다.
- Space 복원은 기존 agent 연결을 끊고 링크 그래프 전체 재생성을 같은 transaction에서 예약한다.
- Folder 영구 삭제는 먼저 별도로 삭제했던 항목을 포함한 물리적 하위 트리 전체에 적용한다.
- Usage는 현재 live counter를 유지하고 복원 시 재검증한다. 실제 저장소 제거 확인과 retained/pending 용량 집계는 별도 계약이다.
- 목록의 `recoverable`은 휴지통 metadata상 복원 후보 여부이며 복원 성공을 보장하지 않는다. `deletion_pending`은 영구 삭제 요청 또는 보관 만료로 DB purge를 기다리는 상태다. S3 삭제 예약·완료 상태는 객체 원장의 `state`로 관리한다. `purge_after`는 purge 가능 시각이며 영구 삭제 요청 시 앞당겨질 수 있고, 실제 물리 삭제 완료 시각이 아니다.

Node/Text/File mutation은 같은 transaction에서 `space_usage` counter를 갱신한다. 생성, 내용 변경, 복사, 이동, soft delete별 증감 규칙은 `usage-and-quotas.md`를 따른다.

## API key

### 생성

User caller만 자신이 소유한 Agent의 API key를 만든다.

```text
api_keys(account_id=agent_id, created_by_user_id=owner_user_id) -- agent key
```

- 평문 token은 생성/rotation 응답에서 한 번만 반환한다.
- Agent key는 caller가 소유한 active agent에게만 만든다.
- `expires_at`은 필수이며 미래 시각이어야 한다.
- Agent key TTL은 최대 365일이다.
- Agent account당 live key는 최대 5개다.

### Revoke/rotation

Revoke:

```text
api_keys.revoked_at=now()
api_keys.revoked_by_user_id=caller
api_keys.revoked_reason=optional_reason
```

Rotation은 같은 account에 새 key를 만들고 old key를 같은 transaction에서 `revoked_reason=rotated`로 revoke한다. Old token 원문은 복구하지 않는다.

## Browser session

### 생성

Browser login callback은 authgate authorization-code + PKCE exchange 결과에서 refresh token을 요구한다.

```text
browser_sessions.user_id=user_id
browser_sessions.token_hash=HMAC(session token)
browser_sessions.refresh_token_*=encrypted authgate refresh token
browser_sessions.validated_until=now()+1h
browser_sessions.expires_at=now()+30d
```

- Browser session token 원문은 HttpOnly cookie에만 발급한다.
- Refresh token은 browser client에 노출하지 않고 서버가 암호화 저장한다.
- AuthGate는 refresh token의 canonical state를 관리한다. NoteGate는 브라우저 세션 갱신을 위해 발급받은 값을 보관한다.

### 갱신

요청의 browser session이 `validated_until`을 넘으면 NoteGate는 저장된 refresh token으로 authgate refresh-token grant를 호출한다.

```text
success:
  validated_until=now()+1h
  last_refreshed_at=now()
  refresh_token_* 교체 -- authgate가 rotated refresh token을 반환한 경우

invalid_grant/sub mismatch:
  revoked_at=now()
  revoked_reason='refresh_failed'
  request returns 401

transient authgate/userinfo failure:
  refresh_token_* 교체 -- token exchange 후 userinfo가 실패했고 rotated refresh token을 받은 경우
  refresh_started_at=NULL
  refresh_claim_id=NULL
  validated_until unchanged
  request returns 503
```

`expires_at`은 absolute lifetime이다. 30일이 지나면 refresh를 시도하지 않고 재로그인을 요구한다.

### Logout/revoke

Logout은 local `browser_sessions` row를 revoke하고 browser session cookie를 만료시킨다. 저장된 refresh token은 authgate revoke endpoint에 best-effort로 revoke 요청한다.
