# Event logging spec

이 문서는 NoteGate의 durable operation history 계약을 정의한다. 무엇을 기록하는지, payload에 무엇을 담는지, 어떤 조회 축을 지원하는지를 정한다. DB schema 정본은 `docs/spec/db.md`, payload 보안 원칙은 `docs/spec/security.md`가 정본이다. Repository-level transaction wiring, helper API, rollout 순서는 구현 detail로 둔다.

## Purpose

Event log는 B2C product self-review를 위한 변경 이력이다. 사용자는 자기 계정과 space에 어떤 관리 변경과 파일 변경이 있었는지 확인하고, agent owner는 agent가 수행한 변경을 되돌아본다. Tamper-evident compliance audit log나 금융권 수준의 forensic log는 이 문서의 범위가 아니다.

NoteGate는 관리 변경과 파일트리 변경을 별도 stream으로 기록한다.

```text
audit_events
  account, session, credential, agent, space, connection 관리 이력과 DB 리소스 정리 완료

file_change_events
  file-tree/file content change 이력

command_invocations
  MCP와 Command API의 read·mutation·실패 호출 이력
```

외부 command 호출 자체는 domain mutation stream과 다른 `command_invocations`에 기록한다. 이 표는 MCP와 Command API의 read와 실패 호출도 포함하는 실행 이력이며 현재 state나 mutation history의 source of truth가 아니다.

`invocation_id`는 서버가 MCP/CLI 호출 경계에서 생성하는 UUID다. 호출 이력과 그 호출로 성공한 Changes에 함께 남기며, 한 sequence의 여러 변경은 같은 호출 ID를 갖는다. 삭제·복원의 작업 범위를 결정하는 `operation_id`는 별개다. Invocation 저장은 best-effort이므로 연결된 호출 행이 없을 수도 있고, legacy 행에는 연결 ID를 추측해 넣지 않는다.

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

## Trash operation correlation

- 새 node/subtree 또는 Space 삭제마다 UUID `operation_id`를 생성한다. 같은 transaction에서 domain의 `deletion_operation_id`와 삭제 event의 `operation_id`를 기록한다.
- 복원과 영구 삭제 요청은 각각 새 `operation_id`를 갖고, `metadata.related_deletion_operation_id`로 원래 삭제를 참조한다. 복원은 domain의 삭제 ID를 해제하지만 event 참조는 유지한다. 재삭제는 새 ID를 사용한다.
- `deletion_target_node_id`는 삭제 요청이 직접 대상으로 삼은 노드 ID이며 함께 복원할 범위를 구분한다. `parent_id`와 파일 트리 root는 별개다. `deletion_operation_id`는 삭제 작업 식별자다. 먼저 삭제된 자식의 ID는 상위 폴더 삭제 때 변경하지 않는다.
- 기존 행과 이 계약 범위 밖의 Audit event는 `operation_id=NULL`이다. 새 Changes는 모든 변경에 `operation_id`를 기록한다. 기존 삭제와 event를 시각 또는 리소스 ID로 추측해서 연결하지 않는다. 연결 ID가 없는 복원/영구 삭제 요청은 `related_deletion_operation_id=null`이다.
- ID는 correlation 용도이며 authorization, request idempotency, event cursor 또는 문서 snapshot을 대체하지 않는다. Event retention은 유지하고, 로그가 없어도 domain state에 따라 복원한다.
- 비동기 purge는 semantic row 제거 전에 원래 삭제 ID를 object 원장에 보존한다. 이는 S3 물리 삭제 완료 event/receipt가 아니며 원장도 기존 retention을 따른다.
- DB purge는 실제로 삭제한 node마다 `node.purge`, 마지막 Space 행을 삭제할 때 `space.purge`를 Audit에 같은 transaction으로 기록한다. 내부 root node는 Space 완료에 포함하며 별도 node 기록을 만들지 않는다. 기록 실패는 해당 삭제 batch를 rollback하고, 재시도는 남아 있는 리소스만 처리하므로 완료 기록이 중복되지 않는다.
- 완료 기록의 `operation_id`는 원래 node 삭제 ID이며, 없으면 Space 삭제 ID를 사용한다. 먼저 개별 삭제된 자식의 ID는 유지한다. 둘 다 없으면 NULL로 두며 과거 삭제에 대한 기록을 추측해서 만들지 않는다.
- 이 기록은 `source=system`, `actor_account_id=NULL`, `completion_scope=database`다. 이름·경로·본문·object key 없이 owner/resource/Space/삭제 대상 ID와 node 종류만 저장한다. 삭제된 리소스에 FK로 연결하지 않으며 소유자의 Audit에서 180일 보관한다. S3 삭제 성공이나 저장소 내부 물리 정리 완료, 용량 반환을 의미하지 않는다.
- MCP/CLI invocation과의 직접 연결은 별도이며, Changes의 Text revision 연결은 아래 snapshot 계약을 따른다. `read op=changes`는 저장된 event의 `operation_id`를 반환한다.

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

저장할 때 `purpose`, `space_name`, redacted `input`/`response`는 한 암호화 envelope로 묶는다. 소유자 ID와 snapshot UUID를 AEAD에 바인딩하며, 조회 시 소유권으로 먼저 필터링한 뒤 복호화한다. 식별자, 시간, 호출 경로, tool/op, 결과 및 오류 코드는 조회를 위해 평문으로 유지한다. 기존 평문 행은 `history.encryption` Reconciler가 최대 100행씩 이관하며, 이관 중에는 기존 형식도 읽는다. 암호화 오류를 평문 fallback으로 숨기지 않는다.

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

Audit event는 account, session, credential, agent, space, connection 관리 변경과 비동기 DB 리소스 정리 완료를 기록한다.

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
space.restore
space.purge
node.purge
trash.purge.request

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

node.purge
  completion_scope: "database"
  space_id: uuid
  item_kind: "folder" | "text" | "file"
  deletion_target_node_id: uuid (known only)

space.purge
  completion_scope: "database"
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

node.purge
  resource_type: "node"
  resource_id: node_id

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
item.restore
```

File change event metadata는 제한된 structural fact와 metric만 담는다. 허용 가능한 예:

```text
item_kind: "folder" | "text" | "file"
item_name: string
parent_node_id: uuid
restored_nodes: integer
copied_from_node_id: uuid
parent_node_id_before: uuid
parent_node_id_after: uuid
name_changed: bool
sort_order_changed: bool
external_access_enabled_changed: bool
text_encryption_changed: bool
write_lock_changed: bool
external_access_enabled: bool
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

`file_change_events`는 owner/space/node 조회 축을 column으로 두고 내용 메타데이터는 암호화한다.

```text
file_change_events
  id
  operation_id
  owner_user_id
  created_at
  space_id
  node_id
  actor_account_id
  op_type
  metadata
  private_metadata
  snapshot_id
```

권장 index와 column type은 `docs/spec/db.md`의 Event history tables가 정본이다.

## Retention and deletion

Retention policy:

```text
audit_events: 180 days
file_change_events: 90 days
command_invocations: 90 days
```

각 event table은 retention 조회/삭제를 위한 `created_at` index를 둔다. Purge worker는 `audit_events` 180일, `file_change_events`와 `command_invocations` 90일을 초과한 행을 테이블별 bounded batch로 삭제한다.

## Changes snapshots

- 새 Changes의 문서 이름·수정 이유·크기 등 내용 메타데이터는 전용 HKDF subkey와 AES-GCM으로 암호화한다. AAD는 Space ID와 행별 `snapshot_id` UUID에 묶인다. 순서/cursor용 event ID와 암호화 snapshot 식별자는 별개다. 식별자·시각·고정된 구조 변경 플래그는 조회와 링크 그래프 갱신을 위해 평문으로 둔다.
- 성공한 변경과 snapshot은 같은 트랜잭션에 저장한다. snapshot 기록 실패는 문서 변경도 롤백하며, 내용이 같은 저장은 새 event/version을 만들지 않는다.
- `metadata.source`는 기록 경로, `actor_kind`는 인증된 계정 종류다. 계정이 Agent라는 사실만으로 AI 실행이라고 단정하지 않는다.
- 지원되는 문서 변경에는 `before_revision_id`/`after_revision_id`를 남긴다. 기존 기록의 연결은 추정해서 채우지 않는다. 폴더 단위 변경은 하위 문서 전체의 버전을 나열하지 않는다.
- 목록의 `before_revision_status`/`after_revision_status`는 본문 존재 상태(`current`, `retained`, `unavailable`)다. `*_revision_cleanup_at`은 보관 본문의 정리 시작 가능 시각이며 즉시 삭제를 보장하지 않는다. 이 정보는 본문 접근 권한을 부여하지 않는다.
- `GET /api/v1/me/file-change-events`는 현재 User의 소유 이력을 Space 삭제 뒤에도 90일 보존 기간 내 조회한다. 선택적 `space_id`, `limit`, `cursor`를 받는다. 외부 Agent 접근과 기존 Space별 권한은 바뀌지 않는다.
- 기존 plaintext Changes는 `history.encryption` Reconciler가 최대 100행씩 암호화한다. 이관 중에는 이전 형식도 읽는다. 이미 삭제된 Space의 소유자를 입증할 수 없는 과거 행은 소유 이력에 노출하지 않는다.
