# Database schema

이 문서는 Notegate DB schema의 정본이다.

## Entity overview

```text
crypto_key_epochs
accounts
users
agents
api_keys
audit_events
file_change_events
spaces
space_usage
space_usage_reconcile_jobs
space_usage_reconcile_executions
space_agent_connections
nodes
text_objects
file_objects
file_inline_contents
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
  tier text not null default 'system_max'
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
users.tier: 'tier0' 또는 'system_max'. Application은 신규 user 생성 시 `NOTEGATE_DEFAULT_USER_TIER` 값을 명시적으로 저장한다. DB default 'system_max'는 직접 SQL 삽입을 위한 fallback이다.
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

Browser session cookie 원문은 저장하지 않는다. `token_hash`는 cookie의 opaque session token을 검증하기 위한 HMAC이다. `refresh_token_*` 필드는 authgate refresh token을 암호화 저장한다. AuthGate가 refresh token의 canonical state를 관리하고, Notegate는 브라우저 세션 갱신을 위해 발급받은 값을 보관한다.

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
```

`file_change_events`는 space 안의 파일/폴더/문서 변경을 기록한다. Retention policy는 3 months이며, space 전체 조회와 node별 조회를 위해 별도 index를 둔다. Event payload 규칙은 `docs/spec/event-logging.md`와 `docs/spec/security.md`를 따른다.

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
file_change_events_actor_time_idx(actor_account_id, created_at desc, id desc)
file_change_events_retention_idx(created_at)
```

## Space and connection tables

```text
spaces
  id uuid pk
  owner_user_id uuid not null references users(id)
  name text not null
  sort_order int not null default 0
  created_at timestamptz
  updated_at timestamptz
  deleted_at timestamptz null
  deleted_by_user_id uuid null references users(id)
  purge_after timestamptz null
```

Live space name은 `(owner_user_id, name)` 기준 unique다. Space name은 1~63자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, `:`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다. Space 목록 기본 정렬은 `(sort_order, name, id)`다. 서비스 생성 경로는 새 space를 `max(owner live sort_order)+1000`으로 만들어 기본적으로 목록 끝에 추가한다. `deleted_at`, `deleted_by_user_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.

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
- `deleted_at`, `deleted_by_account_id`, `purge_after`는 모두 NULL이거나 모두 non-NULL이다.
- Full path는 저장하지 않는다.

```text
text_objects
  node_id uuid pk
  space_id uuid not null references spaces(id) on delete cascade
  storage_format text not null check ('plain','encrypted')
  content_text text null
  encrypted_payload jsonb null
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
  storage_kind text not null check ('inline_pg','object')
  object_key text null
  media_type text not null
  byte_len bigint not null check 0..104857600
  content_sha256 text null
  original_filename text null
  encryption_mode text not null check ('none','client')
  encryption_metadata jsonb null
  uploaded_at timestamptz
```

```text
file_inline_contents
  node_id uuid pk
  space_id uuid not null
  bytes bytea not null check octet_length(bytes) <= 262144
```

`File`은 공통 metadata와 실제 bytes를 분리한다. `inline_pg`는 `file_inline_contents.bytes`에 최대 262144 bytes를 저장한다. `object`는 S3 호환 저장소에 bytes를 저장하고 Notegate는 외부에 노출하지 않는 `object_key`를 저장한다.

Space content quota는 `space_usage.live_text_bytes`와 `space_usage.live_file_bytes`로 독립 검사한다. Text는 `text_objects.byte_len`, File은 `file_objects.byte_len`을 사용한다. `storage_kind='inline_pg'`와 `storage_kind='object'`는 같은 File counter에 포함한다. Soft-deleted node의 bytes는 live quota에 포함하지 않는다.

```text
storage_kind='inline_pg' -> file_inline_contents row가 같은 transaction에서 생성됨, object_key IS NULL, byte_len <= 262144
storage_kind='object'    -> object_key IS NOT NULL, bytes는 S3 호환 저장소에 저장됨
```

```text
object_storage_objects
  id uuid pk
  object_key text unique not null
  space_id/parent_node_id/node_id/requested_by_account_id uuid null
  name/declared_byte_len/media_type/encryption metadata
  state text check ('uploading','attached','expire_pending','expired','delete_pending','deleted')
  last_activity_at/retry_count/retry_after/last_error_code
  created_at/attached_at/delete_requested_at/deleted_at
```

`object_storage_objects`는 업로드 연결과 물리 삭제 재시도를 위한 운영 원장이다. Node/Space soft delete transaction은 연결된 object를 즉시 `delete_pending`으로 전환한다. Hard purge의 같은 전환은 이전에 누락된 요청을 보정하는 안전장치다. 원장은 Node/Space purge 뒤에도 남도록 참조 FK가 `ON DELETE SET NULL`이며, `expired`/`deleted` 이력은 90일 뒤 bounded batch로 삭제한다. `expire_pending`과 `delete_pending`은 S3 삭제 실패를 재시도하는 중간 상태다.

Content FK invariant:

```text
DB FK: text_objects/file_objects row -> matching nodes(id, space_id) ON DELETE CASCADE
DB FK: file_inline_contents row -> matching file_objects(node_id, space_id) ON DELETE CASCADE
DB CHECK: file_inline_contents.bytes <= 262144
DB CHECK: file_objects.byte_len <= 104857600
DB CHECK: inline_pg는 byte_len <= 262144, object_key IS NULL, content_sha256 IS NOT NULL
DB CHECK: object는 object_key IS NOT NULL
Service transaction: storage_kind='inline_pg'는 file_inline_contents row를 하나 생성
Service transaction: storage_kind='object' attach는 node, file_objects, usage counter, file change event, 원장 상태를 함께 commit
```

File content encryption은 client-side only다.

```text
encryption_mode='none'   -> encryption_metadata IS NULL
encryption_mode='client' -> encryption_metadata JSON object, bytes는 클라이언트 암호문
```

Text 저장 invariant:

```text
storage_format='plain'     -> content_text IS NOT NULL, encrypted_payload IS NULL
storage_format='encrypted' -> content_text IS NULL, encrypted_payload IS NOT NULL, encrypted_payload는 JSON object
byte_len                  -> 0..1048576
line_count                -> 0..5000
encoding                  -> 'utf-8'만 허용
```

Text 암호화 정책:

- `storage_format='plain'`은 서버가 읽을 수 있는 UTF-8 content다.
- `storage_format='encrypted'`는 client-side encrypted payload다. 서버는 원문과 복호화 키를 저장하지 않는다.
- REST는 encrypted payload 저장/조회가 가능하다.
- MCP `read op=read`, `write op=write/append/patch/edit`, `search op=grep`은 plain Text만 대상으로 한다.
- plain Text의 `content_sha256`, `byte_len`, `line_count`는 plaintext 기준이다.
- encrypted Text의 `content_sha256`, `byte_len`은 서버 canonical JSON serialization 기준이고 `line_count=0`이다.

Node-content invariant:

```text
text_objects row -> matching nodes.kind='text'
file_objects row -> matching nodes.kind='file'
```

DB trigger는 content row가 올바른 node kind에만 붙도록 보장한다. Folder는 content row를 만들지 않는다. Text 생성/쓰기는 service transaction에서 node와 text_objects row를 함께 만든다.
