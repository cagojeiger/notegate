# Event logging spec

이 문서는 NoteGate의 durable operation history 계약을 정의한다. 무엇을 기록하는지, payload에 무엇을 담는지, 어떤 조회 축을 지원하는지를 정한다. DB schema 정본은 `docs/spec/db.md`, payload 보안 원칙은 `docs/spec/security.md`가 정본이다. Repository-level transaction wiring, helper API, rollout 순서는 구현 detail로 둔다.

## Purpose

Event log는 B2C product self-review를 위한 변경 이력이다. 사용자는 자기 계정과 space에 어떤 관리 변경과 파일 변경이 있었는지 확인하고, agent owner는 agent가 수행한 변경을 되돌아본다. Tamper-evident compliance audit log나 금융권 수준의 forensic log는 이 문서의 범위가 아니다.

NoteGate는 관리 변경과 파일트리 변경을 별도 stream으로 기록한다.

```text
audit_events
  account, session, credential, agent, space, connection 관리 이력

file_change_events
  file-tree/file content change 이력

command_invocations
  MCP와 Command API의 read·mutation·실패 호출 이력
```

외부 command 호출 자체는 domain mutation stream과 다른 `command_invocations`에 기록한다. 이 표는 MCP와 Command API의 read와 실패 호출도 포함하는 실행 이력이며 현재 state나 mutation history의 source of truth가 아니다.

두 mutation stream(`audit_events`, `file_change_events`)은 성공적으로 commit된 domain mutation의 이력이다. 현재 state의 source of truth는 normalized domain table이다. `command_invocations`는 실행 관찰 이력이며 이 mutation 보장에 포함되지 않는다.

Event 조회는 REST로 제공한다. Audit event는 `GET /api/v1/me/audit-events`, command 실행 이력은 `GET /api/v1/me/command-invocations`로 조회하고, file change history는 `GET /api/v1/spaces/{space_id}/file-change-events`, UI forward sync는 `GET /api/v1/spaces/{space_id}/file-change-sync`로 조회한다. Read 계약은 `docs/spec/rest/events.md`에 둔다.

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

`command_invocations`는 domain transaction 밖에서 best-effort로 저장한다. 기록 실패는 이미 수행된 read/mutation 결과를 실패로 바꾸지 않으며 warning log를 남긴다. 인증을 통과한 MCP `tools/call`과 `POST /cli` 요청은 command별 입력 역직렬화 전에 기록 경계를 통과하므로 성공, 업무 오류, `purpose` 오류와 argument schema 오류를 실행 이력에 포함한다. Unknown tool도 포함한다. JSON-RPC `tools/call` 또는 CLI envelope로 해석되지 못한 요청, 인증 전에 거부된 요청, client에서 schema 검증으로 차단되어 전송되지 않은 요청은 caller를 확정할 수 없거나 command 경계에 도달하지 않으므로 포함하지 않는다.

## Command invocation history

이 문서에서 `redaction`은 민감한 원문 값을 제거하거나 redaction marker로 대체하는 처리를 뜻한다. 허용되지 않은 field를 통째로 제외하는 것은 `omission`이다. 일부 문자를 남기는 `masking`과 범위가 모호한 `sanitization`은 이 기능의 용어로 사용하지 않는다.

`command_invocations`는 `owner_user_id`, 실제 `actor_account_id`, user/agent 구분, 호출 경계인 `surface`, 정규화된 `tool`과 optional `op`, `purpose`, redacted `input`/`response` JSON object, success/error, 안정적인 error code, 실행 시간을 저장한다. `surface`는 MCP `tools/call`이면 `mcp`, `POST /cli`이면 `cli`다. Browser History는 두 surface를 독립 tab으로 표시한다.

`read op=changes`는 어느 Space의 변경 stream을 조회했는지 목록에서 바로 확인할 수 있도록 검증된 `space_name` snapshot도 함께 저장한다. `me`는 purpose 예외이므로 NULL이다. 유효한 다른 command의 purpose는 1..200자의 짧은 호출 이유이며, purpose 검증 실패 행에서는 summary purpose가 NULL이고 `input.purpose`는 원문 대신 redaction marker다. Unknown MCP tool/op의 원문은 별도 summary column에 저장하지 않는다.

`input`과 `response`는 실제 실행/응답 객체와 분리된 저장 전용 복사본이다. Tool/op별 allowlist는 purpose, target/path, 구조적 flag/count/hash처럼 분석에 필요한 값만 유지한다. Text `content`, patch/edit 문자열과 `diff`, grep 일치 줄, 검색어, 모든 cursor, 원본 파일명과 암호화 metadata, multipart ETag, presigned URL/header, PII와 자유 형식 오류 문구는 `{"_redacted":true,"category":"..."}` marker로 대체한다. 알려지지 않은 field의 이름과 값은 저장하지 않고 `_omitted_field_count`만 남긴다. 각 snapshot은 redaction 후 256 KiB를 넘으면 전체를 크기 marker로 대체한다.

MCP `response`는 protocol `ErrorData` 또는 `structured_content`에서 만들며 RMCP가 같은 JSON을 복제하는 wire `content[].text`와 `_meta`는 저장하지 않는다. CLI response와 구조화 오류는 같은 저장 전용 JSON 정책으로 정규화한다. Sequence tool은 한 invocation row만 만들고 commands/results에 재귀 redaction을 적용하며 내부 command별 행은 만들지 않는다. Response snapshot이 없는 행은 `response=NULL`이고 모든 행은 90일 retention을 따른다. 호출 이력 조회용 MCP/CLI command는 없으며 user browser의 History > MCP 또는 History > CLI에서 자기 소유 범위만 조회한다.

## Audit event sources

Audit event의 `source`는 mutation을 발생시킨 product surface를 나타낸다.

```text
rest
mcp
system
```

`system`은 internal worker 또는 maintenance action에만 사용한다.

## Audit events

Audit event는 account, session, credential, agent, space, connection 관리 변경을 기록한다.

Audit event type:

```text
account.create
account.delete

session.login
session.logout
session.revoke

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
  reason: bounded enum/string when already part of the domain model

session.revoke
  reason: "refresh_failed"
```

Audit event target mapping:

```text
account.delete
  resource_type: "account"
  resource_id: account_id

account.create
  resource_type: "account"
  resource_id: account_id

session.*
  resource_type: "browser_session"
  resource_id: browser_session_id

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

File change event는 space 안의 파일/폴더/문서 변경 이력을 기록한다. Space 내부 mutation sequence는 `id`로 식별하고 REST self-review history는 `created_at desc, id desc` 순서로 표시한다. Transport surface(REST/MCP/Browser), API key id, request id, IP, user agent 같은 request/security context는 기록하지 않는다. 조회는 space scope이며, 특정 node만 보려면 `node_id` query로 필터링한다.

File change event type:

```text
folder.create
text.create
file.create

text.write
text.append
text.patch
text.edit

item.move
item.update
item.copy
item.delete
```

File change event metadata는 제한된 structural fact와 metric만 담는다. 허용 가능한 예:

```text
item_kind: "folder" | "text" | "file"
item_name: string
parent_node_id: uuid
copied_from_node_id: uuid
parent_node_id_before: uuid
parent_node_id_after: uuid
name_changed: bool
sort_order_changed: bool
search_enabled_changed: bool
text_encryption_changed: bool
write_lock_changed: bool
search_enabled: bool
text_encryption_enabled: bool | null
write_locked: bool
recursive: bool
copied_nodes: integer
copied_texts: integer
copied_files: integer
deleted_nodes: integer
byte_len_before: integer
byte_len_after: integer
line_count_before: integer
line_count_after: integer
```

Create/text/update event는 현재 `parent_node_id`를 기록한다. Move는
`parent_node_id_before`와 `parent_node_id_after`, delete는
`parent_node_id_before`를 기록한다. 이 값은 UI delta sync의 cache
invalidation 범위이며 전체 path는 저장하지 않는다.

`item_name`은 변경 시점의 node name만 저장한다. Content body나 전체 path는 event metadata에 저장하지 않는다.

Agent 기준 검토는 `actor_account_id`에서 시작한다. API key 단위 추적은 현재 file change history 범위에 포함하지 않는다.

File change event target mapping:

```text
folder.create | text.create | file.create | text.* | item.*
  space_id: space_id
  node_id: target node_id

item.copy
  node_id: new node_id
  metadata.copied_from_node_id: source node_id

recursive item.delete
  node_id: root deleted node_id
  metadata.deleted_nodes: deleted node count
```

새 event type을 추가할 때는 이 문서의 allowlist, DB module의 typed payload constructor와 repository wrapper, payload unit test를 같은 변경에서 갱신한다.

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
audit_events: 365 days
file_change_events: 90 days
command_invocations: 90 days
```

각 event table은 retention 조회/삭제를 위한 `created_at` index를 둔다. Purge worker는 `audit_events` 365일, `file_change_events`와 `command_invocations` 90일을 초과한 행을 테이블별 bounded batch로 삭제한다.
