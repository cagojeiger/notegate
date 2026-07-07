# Event logging spec

이 문서는 Notegate의 durable operation history 계약을 정의한다. 무엇을 기록하는지와 목표 DB schema 형태를 정한다. Repository-level transaction wiring, helper API, rollout 순서는 구현 detail로 둔다.

## Purpose

Notegate는 두 append-only event stream을 둔다.

```text
audit_events
  security, credential, permission, account, agent, space 관리 이력

content_events
  file-tree와 content domain operation 이력
```

두 stream은 audit review, incident investigation, agent 변경 검토, 향후 activity view, 향후 usage projection을 지원한다. 현재 state의 source of truth는 아니다.

## Non-goals

- Notegate state 전체 event sourcing.
- Text 또는 file content version history.
- Request/latency logging.
- 첫 범위에서 failed login, validation failure, permission denied, brute-force security event 수집.
- Raw request/response payload 저장.

## Common rules

- Commit에 성공한 domain mutation만 기록한다.
- State change와 같은 DB transaction 안에서 event row를 insert한다.
- Event row는 append-only로 다룬다.
- Resource identifier는 snapshot으로 저장한다. Event row는 이후 product row purge/anonymization 뒤에도 남아야 하므로 target column은 cascading foreign key가 아니라 identifier로 취급한다.
- 자주 필터링하거나 pagination에 쓰는 값만 column으로 둔다. Event별 세부 값은 `metadata`에 둔다.
- `resource_type`/`resource_id`는 event의 primary target이다. Secondary target id는 `metadata`에 둔다.
- `metadata`는 allowlist 기반이고 작아야 한다.
- Secret, token material, raw content, user PII를 저장하지 않는다.

## Capture guarantee

Event capture는 domain mutation의 일부다.

```text
audit_events insert 실패   => 원래 audit 대상 mutation도 실패
content_events insert 실패 => 원래 content 대상 mutation도 실패
```

이 보장은 operation history가 현재 domain state와 어긋나지 않게 하기 위한 기본 계약이다.

Event payload에 절대 저장하지 않는 값:

```text
secret values
bearer tokens
OAuth codes
PKCE verifiers
API key plaintext
API key hashes
browser session tokens
OAuth refresh tokens
auth headers
text content
file bytes
user email
user display name
```

## Event sources

`source`는 mutation을 발생시킨 product surface를 나타낸다.

```text
rest
mcp
system
```

`system`은 internal worker 또는 maintenance action에만 사용한다.

## Audit events

Audit event는 access boundary, credential, security-relevant management state 변경을 기록한다.

초기 audit event type:

```text
account.delete

space.create
space.update
space.delete

agent.create
agent.delete

user_key.create
user_key.rotate
user_key.revoke

agent_key.create
agent_key.rotate
agent_key.revoke

connection.upsert
connection.disconnect
```

Audit event는 read, search, browser session refresh, health probe, static web request를 기록하지 않는다.

Audit event metadata는 operation별 allowlist를 따른다. 예:

```text
space.update
  changed_fields: ["name", "sort_order"]

connection.upsert
  permission: "read" | "write"

agent_key.rotate
  rotated_from_key_id: uuid

*.revoke
  reason: sanitized enum/string when already part of the domain model
```

Audit metadata에는 API key token plaintext, token hash, user email, user display name, raw request body를 포함하지 않는다.

## Content events

Content event는 file-tree와 content-domain mutation을 기록한다. Volume, retention, agent 변경 검토, 향후 projection 요구가 audit event와 다르기 때문에 별도 stream으로 둔다.

초기 content event type:

```text
node.folder.create
node.text.create
node.file.create

node.text.write
node.text.append
node.text.patch
node.text.edit

node.metadata.replace
node.metadata.patch

node.move
node.update
node.copy
node.delete
```

Content event는 text body, file bytes, full node metadata를 저장하지 않는다. 제한된 structural fact와 metric만 저장할 수 있다.

허용 가능한 content metadata 예:

```text
node_kind: "folder" | "text" | "file"
api_key_id: uuid
parent_node_id_before: uuid
parent_node_id_after: uuid
name_changed: bool
sort_order_changed: bool
recursive: bool
copied_nodes: integer
deleted_nodes: integer
byte_len_before: integer
byte_len_after: integer
line_count_before: integer
line_count_after: integer
```

`content_sha256_before`, `content_sha256_after`는 conflict investigation 또는 향후 content projection에 필요할 때만 허용한다. 이 값은 content-derived metadata로 취급하고 넓게 노출하지 않는다.

## Database schema

Schema는 별도 physical table을 사용한다. 두 stream은 공통 event column을 공유하지만 domain-specific target과 payload는 분리한다.

향후 tamper-evidence를 붙일 수 있도록 두 table은 stable ordering과 replay/checkpoint에 필요한 공통 형태를 유지한다.

```text
id            = stream 안의 DB-generated ordering
event_id      = 외부 참조와 idempotency를 위한 unique identifier
occurred_at   = DB timestamp 기준 발생 시각
schema_version = payload 해석 version
```

### `audit_events`

```text
audit_events
  id bigserial pk
  event_id uuid not null unique
  occurred_at timestamptz not null default now()

  actor_account_id uuid null
  owner_user_id uuid null
  source text not null check ('rest','mcp','system')
  op_type text not null

  resource_type text null
  resource_id uuid null

  metadata jsonb not null default '{}'
  schema_version integer not null default 1
```

권장 index:

```text
audit_events_owner_time_idx(owner_user_id, occurred_at desc, id desc)
audit_events_actor_time_idx(actor_account_id, occurred_at desc, id desc)
audit_events_resource_time_idx(resource_type, resource_id, occurred_at desc, id desc)
audit_events_retention_idx(occurred_at)
```

### `content_events`

```text
content_events
  id bigserial pk
  event_id uuid not null unique
  occurred_at timestamptz not null default now()

  actor_account_id uuid null
  owner_user_id uuid null
  source text not null check ('rest','mcp','system')
  op_type text not null

  space_id uuid not null
  resource_type text not null default 'node'
  resource_id uuid null

  metadata jsonb not null default '{}'
  schema_version integer not null default 1
```

권장 index:

```text
content_events_owner_time_idx(owner_user_id, occurred_at desc, id desc)
content_events_actor_time_idx(actor_account_id, occurred_at desc, id desc)
content_events_space_time_idx(space_id, occurred_at desc, id desc)
content_events_resource_time_idx(resource_type, resource_id, occurred_at desc, id desc)
content_events_retention_idx(occurred_at)
```

Agent나 API key 기준 검토는 `actor_account_id`와 metadata의 `api_key_id`로 시작한다. 특정 API key 기준 조회가 주요 workflow가 되면 `api_key_id`를 column/index로 승격할 수 있다.

향후 usage projection이 content event에 의존하게 되면 typed delta column을 추가하거나 projection 전용 table을 둔다. 초기 event history schema는 usage 집계를 위해 column을 미리 늘리지 않는다.

## Retention and deletion

기본 retention:

```text
audit_events: 1 year
content_events: 3 months
```

Event row는 identifier를 보존하되 embedded PII를 피하도록 설계한다. User anonymization 이후에도 attribution shell은 유지하되 개인 정보를 노출하지 않는 것이 목표다.

향후 policy가 event anonymization을 요구하면, event metadata에 PII를 추가하지 않고 actor/owner identifier를 policy에 맞게 clear 또는 replace한다.

## Future scopes

Deferred work:

```text
usage projection from content_events
reconciliation between content_events and source tables
retention purge enforcement
tamper-evidence checkpoints
```
