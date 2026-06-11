# Database schema

이 문서는 새 도메인 모델의 DB 정본이다. 배포 전 리팩토링에서는 기존 migration을 보존하지 않고 단일 초기 migration으로 정리할 수 있다.

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

평문 token은 저장하지 않는다. `scopes`는 현재 빈 배열만 허용한다.

## Space and connection tables

```text
spaces
  id uuid pk
  owner_user_id uuid not null references users(id)
  name text not null
  created_at timestamptz
  updated_at timestamptz
  deleted_at timestamptz null
  deleted_by_user_id uuid null references users(id)
  purge_after timestamptz null
```

Live space name은 `(owner_user_id, name)` 기준 unique다.

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

Connection은 agent 전용이다. User-to-user membership은 초기 제품에 없다.

## Tree and content tables

```text
nodes
  id uuid pk
  space_id uuid not null references spaces(id)
  parent_id uuid null
  name text not null
  kind text not null check ('folder','text','file')
  sort_order int not null default 0
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

```text
text_objects
  node_id uuid pk references nodes(id)
  space_id uuid not null references spaces(id)
  storage_format text not null check ('plain','encrypted')
  content_text text null
  content_ciphertext bytea null
  nonce bytea null
  enc_key_id text null references crypto_key_epochs(key_id)
  enc_version int null
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
  inline_bytes bytea null
  object_key text null
  media_type text not null
  byte_len bigint not null
  content_sha256 text not null
  original_filename text null
  uploaded_at timestamptz
  enc_key_id text null references crypto_key_epochs(key_id)
  enc_version int null
  nonce bytea null
```

`File`은 작은 content를 PostgreSQL `bytea`에 저장할 수 있다. `byte_len <= 262144`이면 `storage_kind='inline_pg'`를 사용할 수 있고, 그보다 큰 파일은 object storage 구현 시 `storage_kind='object'`로 저장한다.

```text
storage_kind='inline_pg' -> inline_bytes IS NOT NULL AND object_key IS NULL
storage_kind='object'    -> inline_bytes IS NULL AND object_key IS NOT NULL
```

Text 저장 invariant:

```text
storage_format='plain'     -> content_text IS NOT NULL, content_ciphertext/nonce/enc_key_id/enc_version IS NULL
storage_format='encrypted' -> content_text IS NULL, content_ciphertext/nonce/enc_key_id/enc_version IS NOT NULL
```

File/Text 공통 암호화 정책:

- Content 암호화용 컬럼은 server-side encryption을 위해 예약되어 있다.
- 현재 REST/MCP read/write/patch/grep surface는 plain Text만 지원한다.
- `content_sha256`, `byte_len`, `line_count`는 plaintext 기준 metadata다.

Node-content invariant:

```text
nodes.kind='folder' -> text_objects/file_objects row 없음
nodes.kind='text'   -> text_objects row 1개
nodes.kind='file'   -> file_objects row 1개
```

이 invariant는 service transaction에서 보장하고, 필요하면 trigger로 보강한다.
