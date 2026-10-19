# Database schema

이 문서는 migration이 정의한 현재 NoteGate DB schema를 설명한다. 실행 가능한 정본은 `backend/crates/db/migrations`다.

## Deployment and rollback

Migration은 forward-only로 운영한다.

- `all`과 `api` process는 startup에서 pending migration을 적용한다.
- `worker`, `reconciler`와 `search` process는 현재 binary가 아는 migration의 적용 여부와 checksum을
  read-only로 검증한다.
- 호환되지 않는 schema 변경은 구 API·worker·reconciler를 모두 중지한 뒤 적용한다. 이전 이미지만 다시 실행하지 않는다.
- Migration 59 전에는 v0.1.112에서 history encryption을 완료하고 복구 가능한 백업을 확인한다. 평문 Changes·호출 payload·수정 사유가 남아 있으면 DDL 전에 중단한다.
- Migration 60은 browser session의 중복 전환 표식만 제거한다. 기존 token·issuer·만료·철회·Audit 행은 보존하며, issuer가 없는 세션은 계속 거부한다.
- Migration이 포함된 release는 forward fix로 복구한다.
- Schema 복원이 필요한 rollback은 배포 전에 검증한 database restore 절차로 수행한다.

## Security tables

Domain마다 active epoch는 하나다. `verify_tag`는 root key 원문 저장 없이 설정과 DB registry 일치를 검증한다.

## Actor tables

`accounts`는 인증과 attribution의 공통 actor다.

외부 신원은 검증된 `issuer`와 subject의 HMAC으로 찾는다. Email이 같아도 계정을 자동 연결하지 않는다.
조회와 저장은 `external_identities` 한 경로를 사용하며, 최초 로그인은 issuer-scoped hash로
transaction lock을 잡는다. Primary key는 동일 신원의 중복 소유자를 차단한다.
삭제 보류 계정의 연결도 retention 동안 유지하고 purge가 PII와 함께 제거한다.


Agent는 user가 관리한다. Agent name은 제품 메타데이터이며 PII 저장소로 사용하지 않는다.

## Credential table

평문 token은 저장하지 않는다. `scopes`는 빈 배열만 허용한다.

## Browser session table

Browser session cookie 원문은 저장하지 않는다. `token_hash`는 cookie의 opaque session token을 검증하기 위한 HMAC이다. 신규 세션은 검증한 `oidc_issuer`에 연결한다. Issuer가 없거나 현재 설정과 다르면 인증을 거부한다. 외부 token과 refresh 상태는 보관하지 않는다. `idle_expires_at`은 미사용 만료이며 `expires_at`은 연장할 수 없는 절대 만료다. 기존 `validated_until`은 migration 61에서 값 변경 없이 이름을 바꾼다.

`client_environment`는 nullable 브라우저·OS 표시다. `created_at`·`last_used_at`·두 만료와 함께 사용자 세션 관리에 사용한다. Raw User-Agent·IP·장비 지문을 저장하지 않는다.

## Local OAuth tables

| 테이블 | 책임·핵심 연결 |
|---|---|
| `oidc_login_flows` | 5분짜리 OIDC state; browser cookie HMAC, issuer, 암호화된 PKCE/nonce/next |
| `oauth_authorizations` | browser-bound 동의 요청과 1회용 code; client/resource/redirect/S256 challenge 및 nullable 승인 user ID |
| `oauth_grants` | user FK, client/resource/scope, absolute/idle expiry, revoked_at, nullable client_environment·last_token_at; 사용자당 활성 100개 |
| `oauth_credentials` | access/refresh HMAC PK와 key epoch; grant FK `ON DELETE CASCADE`, expires_at, consumed_at |
| `oauth_device_codes` | device/user-code HMAC, CLI client/resource/scope, 승인 user FK, polling·거절·소비 상태 |

Bearer token·code·PKCE verifier 원문은 평문 DB 필드에 남기지 않는다. Grant 발급·code 소비·credential 삽입·승인 audit는 함께 commit한다. Refresh 재사용에 의한 철회는 오류 응답 전에 commit한다. 계정 삭제는 grant를 함께 철회한다. 만료·철회는 조회 시 바로 적용하며 purge는 short-lived flow와 15일 이상 지난 dead grant를 제한된 batch로 물리 삭제한다. 만료된 access HMAC은 별도 batch로 바로 정리하고, 사용된 refresh HMAC은 해당 grant의 재사용 탐지를 위해 보존한다.

## Event history tables

Event history table은 현재 상태의 source of truth가 아니다. 성공한 domain mutation의 append-only snapshot history다. Actor, owner, target id는 product row를 직접 소유하지 않는 identifier snapshot이며 cascading foreign key로 다루지 않는다. `actor_account_id`는 mutation caller이고, `owner_user_id`는 audit event가 속한 user-owned product scope다. Audit event의 primary target은 `resource_type`/`resource_id`이고, file change event의 primary target은 `space_id`/`node_id`다. Secondary target id는 `metadata`에 둔다.

`audit_events`는 account, browser session, credential, agent, space, connection 관리 변경과 node/Space DB 정리 완료를 기록한다. 완료 기록은 원본 삭제와 같은 transaction에 저장하며 리소스 삭제 뒤에도 identifier snapshot으로 180일 보존한다. `object.delete`는 별도로 S3 삭제 acknowledgement를 기록한다. Payload와 retention 계약은 `docs/spec/event-logging.md`와 `docs/spec/security.md`를 따른다.

`file_change_events`는 space 안의 파일/폴더/문서 변경을 기록하며 space 전체 조회와 node별 조회를 위한 index를 둔다. Payload와 retention 계약은 `docs/spec/event-logging.md`와 `docs/spec/security.md`를 따른다.

`operation_id`는 새 Changes 및 휴지통 lifecycle 작업마다 생성하는 UUID이며 event의 순서/cursor인 `id`와 별개다. `nodes`/`spaces.deletion_operation_id`는 현재 삭제 작업을 식별한다. `nodes.deletion_target_node_id`는 해당 노드를 포함한 삭제 요청이 직접 대상으로 삼은 노드 ID다. 대상 노드는 자기 ID를, 함께 삭제한 자손은 같은 대상 ID를 저장한다. 직접 부모나 파일 트리의 root를 뜻하지 않으며, 먼저 개별 삭제했던 자손의 값은 변경하지 않는다. 이 식별자에는 FK를 두지 않으며 기존 행은 NULL로 유지한다.

`command_invocations`는 domain event와 분리된 MCP·CLI·외부 API v2 실행 이력이다. 저장 대상과 redaction, 크기 제한, retention 계약은 `docs/spec/event-logging.md`가 소유한다.

호출 기록의 문서 관련 필드는 `private_payload`에 암호화해서 저장하고 조회 시 복호화한다. `invocation_id`는 Changes와의 논리적 연결이고 retention 또는 기록 실패로 연결 대상이 없을 수 있어 FK를 두지 않는다.

기록 경계에서 발생한 실패도 저장할 수 있도록 `op`와 암호화 payload의 `purpose`·`response`는 생략할 수 있다. `surface`는 서버가 판정한 호출 경계이며 client 신원을 뜻하지 않는다. Column별 의미와 정규화 규칙은 `docs/spec/event-logging.md`를 따른다.

## Space and connection tables

Live space name은 `(owner_user_id, name)` 기준 unique다. Space name은 1~63자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, `:`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다. Space 목록 기본 정렬은 `(sort_order, name, id)`다. 서비스 생성 경로는 새 space를 `max(owner live sort_order)+1000`으로 만들어 기본적으로 목록 끝에 추가한다. `navigation_pinned_at`은 탐색 영역 고정 상태이고 `user_mcp_enabled_at`은 User MCP 권한 상태이며 서로 독립적이다. `deleted_at`, `deleted_by_user_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.

`purge_last_attempt_at`은 background purge의 Space 순회용 시각이며 삭제 완료/요청 시각이 아니다. Physical child, due node, object 원장의 parent/Space 조회는 별도 index를 사용한다.

`trash_recoverable`은 해당 Space 삭제가 휴지통 복원 보존 정책을 지원하는지 표시한다. 현재 복원이 가능한지는 만료 시각, 영구 삭제 요청, 파일 원장 상태와 현재 제한을 별도로 검증한다.

`space_usage`는 활성 항목 수와 활성 본문 bytes를 보관한다. Byte quota는 `space_storage_usage`의 보관량을 사용한다. Root node는 `live_node_count`에 포함한다. Space 생성은 root node와 usage row를 같은 transaction에서 만든다. File-tree 변경은 예상 delta를 검증하고 source row와 counter를 같은 transaction에서 갱신한다. 정확한 계산과 복구 기준은 `usage-and-quotas.md`를 따른다.

`space_storage_usage`는 이름/본문 없이 소유자와 용량만 저장한다. Text source 및 object 상태 trigger가 갱신하며 Space 제거 뒤 S3 삭제가 끝날 때까지 유지한다. Byte가 0이고 Space와 nonterminal object 참조가 없으면 bounded history cleanup에서 제거한다.

`background_jobs`는 지연 가능한 내부 작업의 원장이다. `queued`와 `running`은 `completed_at=NULL`, terminal 상태는 claim 정보가 없고 `completed_at`이 있어야 한다. 각 claim은 append-only attempt 한 개를 만들며 defer, lease 만료, timeout, panic, 취소, retryable/permanent failure를 구분한다. `attempt_count`는 실행 상한을 소비하는 전체 실행 이력이고 `failure_count`는 오류 관측값이다. Queue의 전달 보장은 at-least-once이고 terminal 행은 90일 뒤 bounded batch로 삭제한다. 상세 상태 전이는 `background-jobs.md`를 따른다.

Connection은 agent 전용이다. User-to-user membership은 제공하지 않는다. `disconnected_at`, `disconnected_by_user_id`는 모두 NULL이거나 모두 non-NULL이다. 같은 owner user 안의 live space와 active agent만 연결하는 규칙은 connection repository transaction에서 검사한다.

## Tree and content tables

- `(parent_id, space_id)`는 `nodes(id, space_id)`를 참조하는 composite FK다(`UNIQUE (id, space_id)`로 보장). parent는 항상 같은 space 안에 있다.
- Root는 `parent_id IS NULL`, `name='/'`, `kind='folder'`, `deleted_at IS NULL`인 node다.
- Non-root node name은 1~128자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다.
- 같은 parent 안 live node name은 unique다.
- `metadata`는 JSON object여야 한다. content가 아니며 암호화 대상이 아니다. 시스템 관리 값이며 외부 caller에게 읽기 전용이다.
- `external_access_enabled`는 node의 직접 외부 접근 설정이다. MCP·API v2는 자신과 모든 조상이 ON일 때만 접근할 수 있다. 유효 상태는 `node_external_access_allowed`로 계산하며 자식의 설정값은 변경하지 않는다.
- `write_locked`는 직접 설정된 쓰기 잠금이다. descendant 상속 상태는 저장하지 않으며 parent chain에서 계산한다.
- `deleted_at`, `deleted_by_account_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.
- Full path는 저장하지 않는다.
- Create, rename, move, copy, file attach는 DB mutation transaction 안에서 최종 derived path의 depth와 byte 상한을 다시 검사한다. Folder subtree를 옮기거나 복사할 때는 모든 live descendant를 포함한다.

`File` metadata는 `file_objects`에 저장하고 실제 bytes는 S3 호환 저장소에 저장한다. NoteGate는 외부에 노출하지 않는 `object_key`만 저장한다. `media_type`은 client 선언값이고 `detected_media_type`은 object bytes에서 감지한 값이다. `NULL`은 아직 감지하지 못한 상태다.

Space content quota는 `space_storage_usage.text_bytes`와 `file_bytes`로 독립 검사한다. 휴지통 Text와 attached/delete_pending object bytes를 포함하며 DB/S3 완료 transaction이 용량을 반환한다. 과거 revision 본문은 별도 예산이다.

`object_storage_objects`는 업로드 연결과 물리 삭제 재시도를 위한 운영 원장이다. Node/Space soft delete는 현재 본문과 연결된 object를 30일 보존한다. 보관 기간 만료 또는 사용자 영구 삭제 요청 이후 hard purge가 object를 `delete_pending`으로 전환한 뒤 semantic rows를 제거한다. Purge는 객체 원장의 `deletion_operation_id`에 원래 node 삭제 ID(없으면 Space 삭제 ID)를 복사한다. 원장은 Node/Space purge 뒤에도 남도록 참조 FK가 `ON DELETE SET NULL`이며, `expired`/`deleted` 이력은 cluster-singleton purge가 180일 뒤 bounded batch로 삭제한다. Retention 조회는 terminal state와 `COALESCE(deleted_at, last_activity_at)` 순서의 partial index를 사용한다. `expire_pending`과 `delete_pending`은 S3 삭제 실패를 재시도하는 중간 상태다.

`usage_space_id`는 최초 Space ID를 보존하고 FK가 NULL이 되어도 유지한다. 이미 소속이 끊긴 legacy object는 NULL이다. S3 삭제 성공 후 `deleted` 전환, 저장 용량 감소, `object.delete` receipt를 하나의 transaction으로 처리한다. Provider 내부 disk GC는 이 완료 기준에 포함하지 않는다.

File content encryption은 client-side only다.

Text 암호화 정책:

- `storage_format='plain'`은 서버가 읽을 수 있는 UTF-8 content다. `at_rest_encryption='server'`이면 DB에는 AEAD ciphertext로 저장한다.
- `storage_format='encrypted'`는 client-side encrypted payload다. 서버는 원문과 복호화 키를 저장하지 않는다.
- REST는 encrypted payload 저장/조회가 가능하다.
- MCP `read op=read`, `write op=write/append/patch/edit`, `search op=grep`은 plain Text만 대상으로 한다. 서버 관리 at-rest 암호화는 서버에서 투명하게 복호화한다.
- plain Text의 `content_sha256`, `byte_len`, `line_count`는 plaintext 기준이다.
- encrypted Text의 `content_sha256`, `byte_len`은 서버 canonical JSON serialization 기준이고 `line_count=0`이다.
- `at_rest_encryption` 변경과 기존 plain Text 본문 변환은 같은 transaction에서 처리한다.
- `at_rest_encryption`은 서버 관리 암호화의 유일한 Text 저장 상태다.

DB trigger는 content row가 올바른 node kind에만 붙도록 보장한다. Folder는 content row를 만들지 않는다. Text 생성/쓰기는 service transaction에서 node와 text_objects row를 함께 만든다.

## Text revision history

`text_objects.revision_*` tracks body attribution and the current editing group independently of metadata updates. `text_revisions` stores immutable encrypted past bodies and precomputed cleanup eligibility. `text_revision_usage` tracks a separate Space history-body budget; deletion releases it transactionally. Current `revision_private_purpose` and historical `private_purpose` encrypt change reasons independently of bodies. All three are detailed in [Text revisions](text-revisions.md).

### Change snapshot persistence

`file_change_events.owner_user_id`는 소유 이력 조회를 위한 변경 당시 소유자 ID이며 resource FK를 두지 않는다. `private_metadata`에는 행/Space에 묶인 암호화 envelope를 저장한다. `metadata`에는 식별자와 구조 플래그만 남긴다. 소유자·시각 인덱스는 삭제된 Space의 이력 조회에 사용한다. 버전 존재 조회는 `text_objects.revision_id` unique index와 `text_revisions` PK를 사용하고 본문은 읽지 않는다.
