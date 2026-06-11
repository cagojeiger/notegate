# Database schema

이 문서는 Notegate DB schema의 정본이다.

## Entity overview

```text
crypto_key_epochs
accounts
users
agents
api_keys
spaces
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
  id uuid pk references accounts(id)
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
  anonymized_at timestamptz null
```

```text
agents
  id uuid pk references accounts(id)
  owner_user_id uuid not null references users(id)
  name text not null
  created_at timestamptz
```

Agent는 user가 관리한다. Agent name은 제품 메타데이터이며 PII 저장소로 사용하지 않는다.

## Credential table

```text
api_keys
  id uuid pk
  account_id uuid not null references accounts(id)        -- 이 key로 인증되는 account
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

Live space name은 `(owner_user_id, name)` 기준 unique다. Space name은 1~63자이며, 첫 글자는 영문/숫자이고 이후 글자는 영문/숫자/`.`/`_`/`-`만 허용한다. Space 목록 기본 정렬은 `(sort_order, name, id)`다.

```text
space_agent_connections
  space_id uuid not null references spaces(id)
  agent_id uuid not null references agents(id)
  permission text not null check ('read','write')
  connected_by_user_id uuid not null references users(id)
  connected_at timestamptz
  disconnected_at timestamptz null
  disconnected_by_user_id uuid null references users(id)
  primary key (space_id, agent_id)
```

Connection은 agent 전용이다. User-to-user membership은 제공하지 않는다. 같은 owner user 안의 live space와 active agent만 연결하는 규칙은 connection repository transaction에서 검사한다.

## Tree and content tables

```text
nodes
  id uuid pk
  space_id uuid not null references spaces(id)
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
- Root는 `parent_id IS NULL`, `name='/'`, `kind='folder'`인 node다.
- 같은 parent 안 live node name은 unique다.
- Full path는 저장하지 않는다.
- `metadata`는 folder/text/file 공통 JSON object다. content가 아니며 암호화 대상이 아니다.

```text
text_objects
  node_id uuid pk references nodes(id)
  space_id uuid not null references spaces(id)
  storage_format text not null check ('plain','encrypted')
  content_text text null
  encrypted_payload jsonb null
  content_sha256 text not null
  byte_len bigint not null
  line_count int not null
  media_type text not null
  encoding text not null default 'utf-8'
  created_by_account_id uuid not null references accounts(id)
  updated_by_account_id uuid not null references accounts(id)
  created_at timestamptz
  updated_at timestamptz
```

```text
file_objects
  node_id uuid pk references nodes(id)
  space_id uuid not null references spaces(id)
  storage_kind text not null check ('inline_pg','object')
  object_key text null
  media_type text not null
  byte_len bigint not null
  content_sha256 text not null
  original_filename text null
  encryption_mode text not null check ('none','client')
  encryption_metadata jsonb null
  uploaded_at timestamptz

file_inline_contents
  node_id uuid pk references file_objects(node_id)
  space_id uuid not null
  bytes bytea not null
```

`File`은 공통 metadata와 실제 bytes를 분리한다. 현재 content 저장 방식은 `storage_kind='inline_pg'`이며, `file_inline_contents.bytes`에 최대 262144 bytes까지 저장한다. 262144 bytes 초과 file은 제품 상한 `file_max_bytes` 안에 있어도 아직 저장하지 않는다.

```text
storage_kind='inline_pg' -> file_inline_contents row가 같은 transaction에서 생성됨, object_key IS NULL
storage_kind='object'    -> 262144 bytes 초과 file 저장 방식으로 예약됨
```

File content invariant:

```text
DB FK: file_inline_contents row -> matching file_objects(node_id, space_id)
DB CHECK: file_inline_contents.bytes <= 262144
Service transaction: storage_kind='inline_pg' creates exactly one file_inline_contents row
Service transaction: current create path never writes storage_kind='object'
```

File content encryption은 client-side only다.

```text
encryption_mode='none'   -> encryption_metadata IS NULL
encryption_mode='client' -> encryption_metadata JSON object, bytes는 클라이언트 암호문
```

Text 저장 invariant:

```text
storage_format='plain'     -> content_text IS NOT NULL, encrypted_payload IS NULL
storage_format='encrypted' -> content_text IS NULL, encrypted_payload IS NOT NULL
```

Text 암호화 정책:

- `storage_format='plain'`은 서버가 읽을 수 있는 UTF-8 content다.
- `storage_format='encrypted'`는 client-side encrypted payload다. 서버는 원문과 복호화 키를 저장하지 않는다.
- REST는 encrypted payload 저장/조회가 가능하다.
- MCP read/write/patch/grep surface는 plain Text만 대상으로 한다.
- plain Text의 `content_sha256`, `byte_len`, `line_count`는 plaintext 기준이다.
- encrypted Text의 `content_sha256`, `byte_len`은 서버 canonical JSON serialization 기준이고 `line_count=0`이다.

Node-content invariant:

```text
text_objects row -> matching nodes.kind='text'
file_objects row -> matching nodes.kind='file'
```

DB trigger는 content row가 올바른 node kind에만 붙도록 보장한다. Folder는 content row를 만들지 않는다. Text 생성/쓰기는 service transaction에서 node와 text_objects row를 함께 만든다.
