# Event logging spec

이 문서는 Notegate의 durable operation history 계약을 정의한다. 무엇을 기록하는지, payload에 무엇을 담는지, 어떤 조회 축을 지원하는지를 정한다. DB schema 정본은 `docs/spec/db.md`, payload 보안 원칙은 `docs/spec/security.md`가 정본이다. Repository-level transaction wiring, helper API, rollout 순서는 구현 detail로 둔다.

## Purpose

Event log는 B2C product self-review를 위한 변경 이력이다. 사용자는 자기 계정과 space에 어떤 관리 변경과 파일 변경이 있었는지 확인하고, agent owner는 agent가 수행한 변경을 되돌아본다. Tamper-evident compliance audit log나 금융권 수준의 forensic log는 이 문서의 범위가 아니다.

Notegate는 관리 변경과 파일트리 변경을 별도 stream으로 기록한다.

```text
audit_events
  account, credential, agent, space, connection 관리 이력

file_change_events
  file-tree/file content change 이력
```

두 stream은 성공적으로 commit된 domain mutation의 이력이다. 현재 state의 source of truth는 normalized domain table이다.

Event 조회는 REST로 제공한다. Audit event는 `GET /api/v1/me/audit-events`로 조회하고, file change event는 `GET /api/v1/spaces/{space_id}/file-change-events`로 조회한다. Read 계약은 `docs/spec/rest/events.md`에 둔다.

## Common rules

- Commit에 성공한 domain mutation만 기록한다.
- State change와 같은 DB transaction 안에서 event row를 insert한다.
- Event row는 append-only로 다룬다.
- Actor, owner, resource identifier는 snapshot으로 저장한다. Event row는 이후 product row purge/anonymization 뒤에도 남아야 하므로 cascading foreign key가 아니라 identifier로 취급한다.
- `actor_account_id`는 mutation caller다. User와 agent 모두 `accounts.id`로 기록한다.
- `owner_user_id`는 event가 속한 user-owned product scope다. Agent 작업이면 agent owner user를 기록한다.
- 자주 필터링하거나 pagination에 쓰는 값만 column으로 둔다. Event별 세부 값은 `metadata`에 둔다.
- Audit event의 primary target은 `resource_type`/`resource_id`다.
- File change event의 primary target은 `node_id`다.
- Secondary target id는 `metadata`에 둔다.
- `metadata`는 operation별 allowlist를 따르며, identifier, enum, count 같은 작은 structural fact만 담는다.
- `metadata` 변경은 additive만 허용한다. Reader는 모르는 key를 무시하고, 기존 key의 의미를 바꾸는 변경은 새 `op_type`으로 기록한다.

## Capture guarantee

Event capture는 domain mutation의 일부다.

```text
audit_events insert 실패 => 원래 audit 대상 mutation도 실패
file_change_events insert 실패  => 원래 file-tree/content mutation도 실패
```

이 보장은 operation history가 현재 domain state와 어긋나지 않게 하기 위한 기본 계약이다.

## Audit event sources

Audit event의 `source`는 mutation을 발생시킨 product surface를 나타낸다.

```text
rest
mcp
system
```

`system`은 internal worker 또는 maintenance action에만 사용한다.

## Audit events

Audit event는 account, credential, agent, space, connection 관리 변경을 기록한다.

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

Audit event metadata는 operation별 allowlist를 따른다. 예:

```text
space.update
  changed_fields: ["name", "sort_order"]

connection.upsert
  permission: "read" | "write"

*.rotate
  created_key_id: uuid

*.revoke
  reason: sanitized enum/string when already part of the domain model
```

Audit event target mapping:

```text
account.delete
  resource_type: "account"
  resource_id: account_id

space.*
  resource_type: "space"
  resource_id: space_id

agent.*
  resource_type: "agent"
  resource_id: agent_account_id

user_key.create | user_key.revoke | agent_key.create | agent_key.revoke
  resource_type: "api_key"
  resource_id: api_key_id

user_key.rotate | agent_key.rotate
  resource_type: "api_key"
  resource_id: old api_key_id
  metadata.created_key_id: new api_key_id

connection.upsert | connection.disconnect
  resource_type: "space"
  resource_id: space_id
  metadata.agent_id: agent_account_id
```

## File change events

File change event는 space 안의 파일/폴더/문서 변경 이력을 기록한다. Transport surface(REST/MCP/Browser), API key id, request id, IP, user agent 같은 request/security context는 기록하지 않는다. 조회는 space scope이며, 특정 node만 보려면 `node_id` query로 필터링한다.

초기 file change event type:

```text
folder.create
text.create
file.create

text.write
text.append
text.patch
text.edit

metadata.replace
metadata.patch

item.move
item.update
item.copy
item.delete
```

File change event metadata는 제한된 structural fact와 metric만 담는다. 허용 가능한 예:

```text
item_kind: "folder" | "text" | "file"
copied_from_node_id: uuid
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

Agent 기준 검토는 `actor_account_id`에서 시작한다. API key 단위 추적은 현재 file change history 범위에 포함하지 않는다.

File change event target mapping:

```text
folder.create | text.create | file.create | text.* | metadata.* | item.*
  space_id: space_id
  node_id: target node_id

item.copy
  node_id: new node_id
  metadata.copied_from_node_id: source node_id

recursive item.delete
  node_id: root deleted node_id
  metadata.deleted_nodes: deleted node count
```

## Storage shape

Schema는 별도 physical table을 사용한다. `audit_events`는 다음 조회 축을 column으로 둔다.

```text
common
  id
  created_at
  owner_user_id
  actor_account_id
  source
  op_type
  metadata

audit_events
  resource_type
  resource_id
```

`file_change_events`는 space/node 기준 조회 축만 column으로 둔다.

```text
file_change_events
  id
  created_at
  space_id
  node_id
  actor_account_id
  op_type
  metadata
```

권장 index와 column type은 `docs/spec/db.md`의 Event history tables가 정본이다.

## Retention and deletion

Retention policy:

```text
audit_events: 1 year
file_change_events: 3 months
```

현재 migration은 retention 조회/삭제를 위한 `created_at` index만 둔다. 실제 삭제 enforcement는 purge 작업 범위에서 구현한다.

User anonymization 이후에도 attribution shell은 유지한다. 향후 policy가 event anonymization을 요구하면 actor/owner identifier를 policy에 맞게 clear 또는 replace한다.
