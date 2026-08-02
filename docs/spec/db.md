# Database schema

이 문서는 NoteGate DB schema의 정본이다.

## Entity overview

```text
crypto_key_epochs
accounts
users
agents
api_keys
audit_events
file_change_events
mcp_invocations
space_link_index_states
spaces
space_usage
space_usage_reconcile_jobs
space_usage_reconcile_executions
space_agent_connections
nodes
node_link_refs
text_objects
file_objects
object_storage_objects
```

## Security tables

```text
crypto_key_epochs
  key_id text pk
  domain text check ('enc','lookup')
  status text check ('active','verify_only','revoked')
  verify_tag text not null
  version int not null
  created_at timestamptz
  activated_at timestamptz null
  retired_at timestamptz null
  revoked_at timestamptz null
```

Domain마다 active epoch는 하나다. `verify_tag`는 root key 원문 저장 없이 설정과 DB registry 일치를 검증한다.

Security DB 제약:

```text
crypto_key_epochs.key_id: 1..127자이며 첫 글자는 영문/숫자, 이후 영문/숫자/`.`/`_`/`-`
crypto_key_epochs.revoked_at: status='revoked'이면 non-NULL, 아니면 NULL
```

## Actor tables

```text
accounts
  id uuid pk
  kind text check ('user','agent')
  display_name_ciphertext bytea null
  display_name_nonce bytea null
  display_name_enc_key_id text null
  display_name_enc_version int null
  is_active bool
  deleted_at timestamptz null
  deleted_by_account_id uuid null references accounts(id)
  created_at timestamptz
  updated_at timestamptz
```

`accounts`는 인증과 attribution의 공통 actor다.

```text
users
  id uuid pk references accounts(id) on delete cascade
  provider_sub_hash text unique null
  provider_sub_hash_key_id text null
  provider_sub_hash_version int null
  email_ciphertext bytea null
  email_nonce bytea null
  email_enc_key_id text null
  email_enc_version int null
  email_hash text null
  email_hash_key_id text null
  email_hash_version int null
  tier text not null default 'tier0'
  anonymized_at timestamptz null
```

```text
agents
  id uuid pk references accounts(id) on delete cascade
  owner_user_id uuid not null references users(id)
  name text not null
  created_at timestamptz
```

Agent는 user가 관리한다. Agent name은 제품 메타데이터이며 PII 저장소로 사용하지 않는다.

Actor DB 제약:

```text
agents.name: 1..63자이며 trim 후 빈 문자열이면 안 됨
accounts.display_name_*: 암호화 display-name 필드는 모두 NULL이거나 모두 non-NULL
accounts.deleted_*: deleted_at과 deleted_by_account_id는 모두 NULL이거나 모두 non-NULL
users.provider_sub_hash_*: provider_sub hash 필드는 모두 NULL이거나 모두 non-NULL
users.email_enc_*: email 암호화 필드는 모두 NULL이거나 모두 non-NULL
users.email_hash_*: email lookup hash 필드는 모두 NULL이거나 모두 non-NULL
users.tier: 'tier0' 또는 'system_max'. Application은 신규 user 생성 시 `NOTEGATE_DEFAULT_USER_TIER` 값을 명시적으로 저장한다. DB default `tier0`는 직접 SQL 삽입을 위한 fallback이다.
```

## Credential table

```text
api_keys
  id uuid pk
  account_id uuid not null references accounts(id) on delete cascade -- 이 key로 인증되는 account
  created_by_user_id uuid not null references users(id)   -- 이 key를 만든 user
  name text not null
  token_prefix text not null
  token_hash text not null unique
  hash_key_id text not null references crypto_key_epochs(key_id)
  hash_version int not null
  scopes text[] not null default '{}'
  created_at timestamptz
  last_used_at timestamptz null
  expires_at timestamptz not null
  revoked_at timestamptz null
  revoked_by_user_id uuid null references users(id)
  revoked_reason text null
  rotated_from_key_id uuid null references api_keys(id)
```

평문 token은 저장하지 않는다. `scopes`는 빈 배열만 허용한다.

Credential DB 제약:

```text
api_keys.name: 1..63자이며 trim 후 빈 문자열이면 안 됨
api_keys.scopes: cardinality(scopes) = 0
api_keys.revoked_*: revoked_at, revoked_by_user_id, revoked_reason은 모두 NULL이거나 모두 non-NULL
api_keys.account_id: Agent account만 참조할 수 있으며 DB trigger가 신규 User-owned key를 거부함
```

## Browser session table

```text
browser_sessions
  id uuid pk
  user_id uuid not null references users(id) on delete cascade
  token_prefix text not null
  token_hash text not null unique
  hash_key_id text not null references crypto_key_epochs(key_id)
  hash_version int not null
  refresh_token_ciphertext bytea not null
  refresh_token_nonce bytea not null
  refresh_token_enc_key_id text not null references crypto_key_epochs(key_id)
  refresh_token_enc_version int not null
  validated_until timestamptz not null
  expires_at timestamptz not null
  last_used_at timestamptz null
  last_refreshed_at timestamptz null
  refresh_started_at timestamptz null
  refresh_claim_id uuid null
  revoked_at timestamptz null
  revoked_reason text null
  created_at timestamptz
  updated_at timestamptz
```

Browser session cookie 원문은 저장하지 않는다. `token_hash`는 cookie의 opaque session token을 검증하기 위한 HMAC이다. `refresh_token_*` 필드는 authgate refresh token을 암호화 저장한다. AuthGate가 refresh token의 canonical state를 관리하고, NoteGate는 브라우저 세션 갱신을 위해 발급받은 값을 보관한다.

Browser session DB 제약:

```text
browser_sessions.refresh_token_enc_*: refresh token 암호화 필드는 모두 non-NULL
browser_sessions.validated_until <= browser_sessions.expires_at
browser_sessions.refresh_* claim: refresh_started_at과 refresh_claim_id는 둘 다 NULL이거나 둘 다 non-NULL
browser_sessions.revoked_reason: revoked_at이 NULL이면 NULL
```

## Event history tables

Event history table은 현재 상태의 source of truth가 아니다. 성공한 domain mutation의 append-only snapshot history다. Actor, owner, target id는 product row를 직접 소유하지 않는 identifier snapshot이며 cascading foreign key로 다루지 않는다. `actor_account_id`는 mutation caller이고, `owner_user_id`는 audit event가 속한 user-owned product scope다. Audit event의 primary target은 `resource_type`/`resource_id`이고, file change event의 primary target은 `space_id`/`node_id`다. Secondary target id는 `metadata`에 둔다.

```text
audit_events
  id bigserial pk
  created_at timestamptz not null default now()
  owner_user_id uuid null
  actor_account_id uuid null
  source text not null check ('rest','mcp','system')
  op_type text not null
  resource_type text not null
  resource_id uuid null
  metadata jsonb not null default '{}'
```

`audit_events`는 account, browser session, credential, agent, space, connection 관리 변경을 기록한다. Retention policy는 1 year이며, 현재 schema는 purge 구현을 위한 `created_at` index까지만 둔다. Event payload 규칙은 `docs/spec/event-logging.md`와 `docs/spec/security.md`를 따른다.

```text
file_change_events
  id bigserial pk
  created_at timestamptz not null default now()
  space_id uuid not null
  node_id uuid null
  actor_account_id uuid null
  op_type text not null
  metadata jsonb not null default '{}'
  link_index_generation bigint null check > 0
```

`file_change_events`는 space 안의 파일/폴더/문서 변경을 기록한다. Retention policy는 3 months이며, space 전체 조회와 node별 조회를 위해 별도 index를 둔다. `link_index_generation`은 마이그레이션 이후 이벤트에 Space별 commit 순서로 부여되는 링크 투영 cursor다. Event payload 규칙은 `docs/spec/event-logging.md`와 `docs/spec/security.md`를 따른다.

`mcp_invocations`는 domain event와 분리된 MCP 실행 이력이다. Tool/op별 allowlist와 redaction policy를 적용하고 크기를 제한한 request/response snapshot을 JSONB로 저장한다.

```text
mcp_invocations
  id bigserial pk
  created_at timestamptz not null default now()
  owner_user_id uuid not null
  actor_account_id uuid not null
  caller_kind text check ('user','agent')
  tool text not null
  op text null
  purpose text null
  space_name text null
  input jsonb not null check object
  response jsonb null check object
  outcome text check ('success','error')
  error_code text null
  duration_ms bigint not null check >= 0
```

유효한 호출의 `purpose`는 앞뒤 공백 없는 1..200자다. `purpose` 자체가 없거나 잘못된 실패도 기록해야 하므로 DB는 NULL을 허용하고, 잘못된 값은 `input`에 redaction marker로 남긴다. `space_name`은 `read op=changes`에만 허용되는 검증된 Space-name summary다. Target/path는 owner self-review를 위해 redacted `input`에 유지한다. Unknown tool은 `tool='unknown'`, 지원하지 않는 op는 `op=NULL`로 정규화한다. `response=NULL`은 response logging 도입 이전의 기존 행을 뜻한다. 성공 행은 `error_code=NULL`, 실패 행은 안정적인 application code, `invalid_params`/`tool_error`, 또는 JSON-RPC code를 가진다. 저장 실패는 원래 MCP 실행 결과를 바꾸지 않는다. User browser는 owner scope로 실행 이력을 조회한다. Retention policy는 90일이며 기존 purge worker가 `created_at` index를 사용해 bounded batch로 삭제한다.

Event history DB 제약:

```text
audit_events.source: 'rest', 'mcp', 'system'
metadata: JSON object
created_at: DB timestamp 기준
```

권장 index:

```text
audit_events_owner_time_idx(owner_user_id, created_at desc, id desc)
audit_events_actor_time_idx(actor_account_id, created_at desc, id desc)
audit_events_resource_time_idx(resource_type, resource_id, created_at desc, id desc)
audit_events_retention_idx(created_at)

file_change_events_space_time_idx(space_id, created_at desc, id desc)
file_change_events_node_time_idx(space_id, node_id, created_at desc, id desc)
file_change_events_space_id_idx(space_id, id)
file_change_events_actor_time_idx(actor_account_id, created_at desc, id desc)
file_change_events_retention_idx(created_at)
file_change_events_link_index_generation_idx(space_id, link_index_generation) unique where non-null

mcp_invocations_owner_time_idx(owner_user_id, created_at desc, id desc)
mcp_invocations_actor_time_idx(actor_account_id, created_at desc, id desc)
mcp_invocations_retention_idx(created_at)
```

## Space and connection tables

```text
spaces
  id uuid pk
  owner_user_id uuid not null references users(id)
  name text not null
  sort_order int not null default 0
  navigation_pinned_at timestamptz null
  user_mcp_enabled_at timestamptz null
  default_search_enabled bool not null default true
  default_text_encryption_enabled bool not null default false
  created_at timestamptz
  updated_at timestamptz
  deleted_at timestamptz null
  deleted_by_user_id uuid null references users(id)
  purge_after timestamptz null
```

Live space name은 `(owner_user_id, name)` 기준 unique다. Space name은 1~63자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, `:`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다. Space 목록 기본 정렬은 `(sort_order, name, id)`다. 서비스 생성 경로는 새 space를 `max(owner live sort_order)+1000`으로 만들어 기본적으로 목록 끝에 추가한다. `navigation_pinned_at`은 탐색 영역 고정 상태이고 `user_mcp_enabled_at`은 User MCP 권한 상태이며 서로 독립적이다. `deleted_at`, `deleted_by_user_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.

```text
space_usage
  space_id uuid pk references spaces(id) on delete cascade
  live_node_count bigint not null default 1 check >= 1
  live_text_bytes bigint not null default 0 check >= 0
  live_file_bytes bigint not null default 0 check >= 0
  reconciled_at timestamptz not null
```

`space_usage`는 일반 쿼터 검사와 Usage 조회를 위한 authoritative counter를 보관한다. Root node는 `live_node_count`에 포함한다. Space 생성은 root node와 usage row를 같은 transaction에서 만든다. File-tree 변경은 예상 delta를 검증하고 source row와 counter를 같은 transaction에서 갱신한다. 정확한 계산과 복구 기준은 `usage-and-quotas.md`를 따른다.

```text
space_usage_reconcile_jobs
  job_id uuid pk
  space_id uuid unique references spaces(id) on delete cascade
  requested_at timestamptz not null
  run_after timestamptz not null
  retry_count integer not null default 0 check >= 0
```

`space_usage_reconcile_jobs`는 수동 요청으로 생성된 활성 작업만 보관한다. `space_id` unique 제약으로 같은 Space의 중복 작업을 막는다. 성공 또는 취소 시 행을 삭제하고, 지연 또는 실패 시 `run_after`를 갱신한다. 전체 재계산은 기존 job을 성공 execution으로 마감한 뒤 삭제한다. `retry_count`는 실패에만 증가한다.

```text
space_usage_reconcile_executions
  execution_id uuid pk
  job_id uuid not null
  space_id uuid not null
  started_at timestamptz not null
  finished_at timestamptz not null
  outcome text check ('succeeded','deferred','failed','cancelled')
  error_message text null
  metadata jsonb not null default '{}'
```

`space_usage_reconcile_executions`는 worker 처리 1회를 append-only로 기록한다. Job은 완료 후 삭제하므로 `job_id`에 FK를 두지 않는다. 실패한 execution만 `error_message`를 가지며, 3개월이 지난 행은 worker가 정리한다.

```text
space_agent_connections
  space_id uuid not null references spaces(id) on delete cascade
  agent_id uuid not null references agents(id) on delete cascade
  permission text not null check ('read','write')
  connected_by_user_id uuid not null references users(id)
  connected_at timestamptz
  disconnected_at timestamptz null
  disconnected_by_user_id uuid null references users(id)
  primary key (space_id, agent_id)
```

Connection은 agent 전용이다. User-to-user membership은 제공하지 않는다. `disconnected_at`, `disconnected_by_user_id`는 모두 NULL이거나 모두 non-NULL이다. 같은 owner user 안의 live space와 active agent만 연결하는 규칙은 connection repository transaction에서 검사한다.

## Tree and content tables

```text
nodes
  id uuid pk
  space_id uuid not null references spaces(id) on delete cascade
  parent_id uuid null
  name text not null
  kind text not null check ('folder','text','file')
  sort_order int not null default 0
  metadata jsonb not null default '{}'
  search_enabled bool not null default true
  write_locked bool not null default false
  created_by_account_id uuid not null references accounts(id)
  updated_by_account_id uuid not null references accounts(id)
  deleted_by_account_id uuid null references accounts(id)
  created_at timestamptz
  updated_at timestamptz
  deleted_at timestamptz null
  purge_after timestamptz null
```

- `(parent_id, space_id)`는 `nodes(id, space_id)`를 참조하는 composite FK다(`UNIQUE (id, space_id)`로 보장). parent는 항상 같은 space 안에 있다.
- Root는 `parent_id IS NULL`, `name='/'`, `kind='folder'`, `deleted_at IS NULL`인 node다.
- Non-root node name은 1~128자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다.
- 같은 parent 안 live node name은 unique다.
- `metadata`는 JSON object여야 한다. content가 아니며 암호화 대상이 아니다.
- `search_enabled`는 해당 node만 검색 결과에 포함할지를 나타낸다. Folder 자식에게 상속되지 않는다.
- `write_locked`는 직접 설정된 쓰기 잠금이다. descendant 상속 상태는 저장하지 않으며 parent chain에서 계산한다.
- `deleted_at`, `deleted_by_account_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.
- Full path는 저장하지 않는다.
- Create, rename, move, copy, file attach는 DB mutation transaction 안에서 최종 derived path의 depth와 byte 상한을 다시 검사한다. Folder subtree를 옮기거나 복사할 때는 모든 live descendant를 포함한다.

```text
text_objects
  node_id uuid pk
  space_id uuid not null references spaces(id) on delete cascade
  storage_format text not null check ('plain','encrypted')
  content_text text null
  encrypted_payload jsonb null
  at_rest_encryption text not null check ('none','server')
  content_ciphertext bytea null
  content_nonce bytea null
  content_enc_key_id text null references crypto_key_epochs(key_id)
  content_enc_version int null
  content_sha256 text not null
  byte_len bigint not null check 0..1048576
  line_count int not null check 0..5000
  media_type text not null
  encoding text not null default 'utf-8' check = 'utf-8'
  created_by_account_id uuid not null references accounts(id)
  updated_by_account_id uuid not null references accounts(id)
  created_at timestamptz
  updated_at timestamptz
```

```text
file_objects
  node_id uuid pk
  space_id uuid not null references spaces(id) on delete cascade
  object_key text not null
  media_type text not null
  detected_media_type text null
  byte_len bigint not null check 0..107374182400
  original_filename text null
  encryption_mode text not null check ('none','client')
  encryption_metadata jsonb null
  uploaded_at timestamptz
```

`File` metadata는 `file_objects`에 저장하고 실제 bytes는 S3 호환 저장소에 저장한다. NoteGate는 외부에 노출하지 않는 `object_key`만 저장한다. `media_type`은 client 선언값이고 `detected_media_type`은 object bytes에서 감지한 값이다. `NULL`은 아직 감지하지 못한 상태다.

Space content quota는 `space_usage.live_text_bytes`와 `space_usage.live_file_bytes`로 독립 검사한다. Text는 `text_objects.byte_len`, File은 `file_objects.byte_len`을 사용한다. Soft-deleted node의 bytes는 live quota에 포함하지 않는다.

```text
object_storage_objects
  id uuid pk
  object_key text unique not null
  space_id/parent_node_id/node_id/requested_by_account_id uuid null
  name/declared_byte_len/media_type/encryption metadata
  upload_mode text check ('single','multipart')
  multipart_upload_id text null
  multipart_part_size bigint null
  state text check ('uploading','attached','expire_pending','expired','delete_pending','deleted')
  last_activity_at/retry_count/retry_after/last_error_code
  created_at/attached_at/delete_requested_at/deleted_at
```

`object_storage_objects`는 업로드 연결과 물리 삭제 재시도를 위한 운영 원장이다. Node/Space soft delete transaction은 연결된 object를 즉시 `delete_pending`으로 전환한다. Hard purge의 같은 전환은 이전에 누락된 요청을 보정하는 안전장치다. 원장은 Node/Space purge 뒤에도 남도록 참조 FK가 `ON DELETE SET NULL`이며, `expired`/`deleted` 이력은 90일 뒤 bounded batch로 삭제한다. `expire_pending`과 `delete_pending`은 S3 삭제 실패를 재시도하는 중간 상태다.

## Link index projection

```text
space_link_index_states
  space_id uuid pk references spaces(id) on delete cascade
  desired_generation bigint not null default 0
  applied_generation bigint not null default 0
  status text check ('queued','running','rebuilding','ready','failed')
  rebuild_requested bool not null default true
  rebuild_base_generation bigint null
  rebuild_after_node_id uuid null
  parser_version int not null default 1
  claim_token uuid null
  claim_until timestamptz null
  retry_count int not null default 0
  run_after timestamptz not null default now()
  last_error text null
  last_indexed_at timestamptz null
  updated_at timestamptz not null default now()

node_link_refs
  id bigserial pk
  space_id uuid not null references spaces(id) on delete cascade
  source_node_id uuid not null
  target_node_id uuid null
  reference_kind text check ('link','image')
  raw_href text not null
  normalized_target_path text null
  occurrence_count int not null check > 0
  indexed_at timestamptz not null default now()
```

`space_link_index_states`는 Space별 durable queue와 재구성 lease를 함께 보관한다. `desired_generation`은 정본 변경 위치이고 `applied_generation`은 현재 링크 투영이 반영한 위치다. `node_link_refs`는 source Text의 현재 outgoing 참조만 저장하며 incoming은 `target_node_id` 역조회로 계산한다. Source와 target composite FK는 같은 Space 안의 node만 허용한다. Target hard purge는 `target_node_id`만 NULL로 바꾸어 unresolved path를 보존한다. 상세 갱신 및 조회 계약은 `docs/spec/link-index.md`가 정본이다.

권장 index:

```text
space_link_index_states_ready_idx(run_after, updated_at, space_id) partial
node_link_refs_outgoing_idx(space_id, source_node_id, id)
node_link_refs_incoming_idx(space_id, target_node_id, id) where target_node_id is not null
node_link_refs_target_path_hash_idx(space_id, md5(normalized_target_path)) partial
nodes_live_text_link_rebuild_idx(space_id, id) where live non-root text
```

Content FK invariant:

```text
DB FK: text_objects/file_objects row -> matching nodes(id, space_id) ON DELETE CASCADE
DB CHECK: file_objects.byte_len <= 107374182400
DB CHECK: client-encrypted file_objects.detected_media_type IS NULL
DB FK: file_objects.object_key -> object_storage_objects.object_key
DB CHECK: file_objects.object_key IS NOT NULL
Service transaction: object attach는 node, file_objects, usage counter, file change event, 원장 상태를 함께 commit
```

File content encryption은 client-side only다.

```text
encryption_mode='none'   -> encryption_metadata IS NULL
encryption_mode='client' -> encryption_metadata JSON object, bytes는 클라이언트 암호문
```

Text 저장 invariant:

```text
storage_format='plain', at_rest_encryption='none'
  -> content_text IS NOT NULL, encrypted_payload와 content 암호화 필드는 NULL
storage_format='plain', at_rest_encryption='server'
  -> content_text와 encrypted_payload는 NULL, content 암호화 필드는 모두 non-NULL
storage_format='encrypted'
  -> at_rest_encryption='none', content_text와 content 암호화 필드는 NULL,
     encrypted_payload는 JSON object
byte_len                  -> 0..1048576
line_count                -> 0..5000
encoding                  -> 'utf-8'만 허용
```

Text 암호화 정책:

- `storage_format='plain'`은 서버가 읽을 수 있는 UTF-8 content다. `at_rest_encryption='server'`이면 DB에는 AEAD ciphertext로 저장한다.
- `storage_format='encrypted'`는 client-side encrypted payload다. 서버는 원문과 복호화 키를 저장하지 않는다.
- REST는 encrypted payload 저장/조회가 가능하다.
- MCP `read op=read`, `write op=write/append/patch/edit`, `search op=grep`은 plain Text만 대상으로 한다. 서버 관리 at-rest 암호화는 서버에서 투명하게 복호화한다.
- plain Text의 `content_sha256`, `byte_len`, `line_count`는 plaintext 기준이다.
- encrypted Text의 `content_sha256`, `byte_len`은 서버 canonical JSON serialization 기준이고 `line_count=0`이다.
- `at_rest_encryption` 변경과 기존 plain Text 본문 변환은 같은 transaction에서 처리한다.
- `at_rest_encryption`은 서버 관리 암호화의 유일한 Text 저장 상태다.

Node-content invariant:

```text
text_objects row -> matching nodes.kind='text'
file_objects row -> matching nodes.kind='file'
```

DB trigger는 content row가 올바른 node kind에만 붙도록 보장한다. Folder는 content row를 만들지 않는다. Text 생성/쓰기는 service transaction에서 node와 text_objects row를 함께 만든다.
