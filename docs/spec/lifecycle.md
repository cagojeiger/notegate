# Lifecycle 정책

이 문서는 notegate에서 주요 리소스를 생성, 삭제, 비활성화할 때 어떤 row가 함께 생성되거나 변경되는지 정의하는 정본이다. DB schema, REST, MCP 문서는 lifecycle의 상세 side effect를 반복하지 않고 이 문서를 따른다.

## 책임 경계

```text
API / MCP layer
  - 인증된 caller를 service layer에 전달한다.
  - request/response 변환을 담당한다.
  - lifecycle side effect를 직접 결정하지 않는다.

Service layer
  - 생성/삭제 lifecycle 정책의 정본 실행 지점이다.
  - 권한 확인, role 판단, side effect 순서, transaction 경계를 결정한다.
  - 여러 row가 함께 바뀌어야 하는 작업은 하나의 transaction으로 묶는다.

DB layer
  - FK, UNIQUE, CHECK, partial index로 구조적 invariant를 방어한다.
  - race condition이 가능한 count/lock 검사를 transaction 안에서 수행한다.
  - lifecycle 정책을 추측해서 보정하지 않는다.

Background job / admin repair
  - purge_after가 지난 soft-deleted row를 hard delete할 수 있다.
  - 깨진 invariant를 감지하고, 명확히 복구 가능한 경우만 별도 repair 경로로 복구한다.
```

## 생성 lifecycle

### Local user 최초 생성

Browser login/onboarding flow로 처음 확인된 local user는 하나의 transaction에서 identity에 필요한 row만 생성한다.

```text
accounts(kind='user')
users
account_encryption_keys
```

규칙:

- user 생성은 workspace, root node, workspace access, agent, agent key를 자동 생성하지 않는다.
- 첫 workspace는 사용자가 REST/MCP의 workspace create를 명시적으로 호출해 만든다.
- REST/MCP bearer 인증은 이미 생성된 local user만 resolve한다. Local user가 없으면 `not_registered`로 거부한다.

### User 재로그인

기존 active local user가 다시 로그인하면 다음만 갱신한다.

```text
accounts display name ciphertext
users email ciphertext/hash
```

재로그인은 workspace, owner access, agent, agent key를 새로 만들지 않는다. 탈퇴로 anonymize된 user는 provider subject lookup 대상이 아니므로 이전 account를 재활성화하지 않고 새 local user 최초 생성 흐름을 탄다.

### Workspace 생성

User caller만 workspace를 생성할 수 있다. Agent caller는 workspace를 생성할 수 없다.

Workspace 생성 transaction은 다음을 함께 만든다.

```text
workspaces
workspace root node '/'
workspace_access(role='owner') for creator user
```

규칙:

- `workspaces.created_by`는 최초 생성자/audit attribution이다.
- 현재 권한 source of truth는 `workspace_access`다.
- creator의 owner access row는 생성 직후 active 상태여야 한다.
- workspace 생성은 user creator account의 live workspace 한도 `20`을 넘을 수 없다.
- root node 생성은 DB trigger로 보장할 수 있지만, lifecycle 관점에서는 workspace 생성 transaction의 일부다.

### Agent 생성

User caller만 agent를 생성할 수 있다.

```text
accounts(kind='agent')
agents
```

규칙:

- agent key는 자동 생성하지 않는다.
- workspace access는 자동 생성하지 않는다.
- agent는 workspace owner가 될 수 없다.
- agent 생성자 user account당 active agent 한도는 `50`이다.

### Agent key 생성

Agent key는 agent 생성과 별도 명시 호출로만 만든다.

```text
agent_keys(token_hash only)
```

규칙:

- 평문 token은 생성 응답에서 정확히 한 번만 반환하고 저장하지 않는다.
- key 생성은 workspace 권한을 변경하지 않는다.
- `scopes`는 생략하거나 빈 배열이어야 한다.
- agent당 live key 한도는 `10`이다.

### Workspace access grant/change

Workspace owner user만 access를 grant/change할 수 있다.

```text
workspace_access insert/update
```

규칙:

- `owner` role은 active user account에만 부여할 수 있다.
- agent account는 `viewer` 또는 `editor`만 받을 수 있다.
- grant/change는 account, workspace, agent key를 새로 만들지 않는다.
- 한 workspace의 active access row 한도는 owner row를 포함해 `20`이다.
- owner 보호 규칙은 항상 적용한다.

### Folder/document 생성

```text
mkdir                 -> nodes(kind='folder')
touch/write(create)  -> nodes(kind='document') + documents
```

규칙:

- file tree node의 위치 정본은 `parent_id + name`이다.
- document 본문은 `documents.content_md`에 원본 Markdown으로 저장한다.
- 생성자는 `created_by`, 마지막 변경자는 `updated_by`에 기록한다.

## 삭제/비활성화 lifecycle

### Workspace 삭제

Workspace 삭제는 active user owner만 수행할 수 있다.

```text
workspaces.deleted_at = now()
workspaces.deleted_by = caller
workspaces.purge_after = now() + retention
```

규칙:

- workspace 삭제는 soft delete다.
- 내부 `workspace_access`, `nodes`, `documents`는 즉시 갱신하지 않는다.
- 모든 조회는 live workspace만 대상으로 하므로 soft-deleted workspace 내부 row는 숨겨진다.
- `purge_after` 이후 background purge가 workspace를 hard delete하면 FK cascade로 내부 row가 제거된다.

### User 탈퇴

User 탈퇴는 hard delete가 아니라 deactivate/anonymize다.

```text
accounts.is_active = false
accounts.deleted_at/deleted_by 설정
users PII ciphertext/hash 제거
account_encryption_keys.wrapped_dek 제거 및 destroyed_at 설정
owned active agents deactivate
owned live agent keys revoke
workspace_access revoke 또는 workspace soft delete
```

Workspace 처리 규칙:

- 탈퇴 user가 유일한 active user owner인 live workspace는 soft delete한다.
- 다른 active user owner가 남는 workspace에서는 탈퇴 user의 access를 revoke한다.
- 탈퇴 user가 만든 agent의 live workspace access도 revoke한다.

### Agent 삭제

Agent 삭제는 agent account의 deactivate/soft delete다.

```text
accounts(kind='agent').is_active = false
accounts.deleted_at/deleted_by 설정
agent_keys revoke
workspace_access revoke
```

`agents` row는 attribution 보존을 위해 일반 product action에서 hard delete하지 않는다.

### Agent key revoke

```text
agent_keys.revoked_at = now()
agent_keys.revoked_by = caller
```

revoke된 key는 인증에 사용할 수 없고 live key 한도 계산에서 제외한다.

### Workspace access revoke

```text
workspace_access.revoked_at = now()
workspace_access.revoked_by = caller
```

규칙:

- 마지막 active user owner는 revoke할 수 없다.
- workspace creator에게 자동 생성된 owner row는 일반 access API로 revoke/downgrade할 수 없다.
- creator owner row 제거가 필요한 경우는 workspace delete 또는 user deletion lifecycle에서만 처리한다.

### File/node 삭제

File tree 삭제는 node soft delete다.

```text
nodes.deleted_at = now()
nodes.deleted_by = caller
nodes.purge_after = now() + retention
```

Document node가 soft delete되면 해당 document는 live file/search 결과에서 제외한다. Hard delete는 purge job 또는 workspace hard delete cascade로 처리한다.

## Owner access 보호 규칙

- Live workspace는 항상 active user owner access row를 최소 1개 가져야 한다.
- Workspace 생성 transaction은 생성 user에게 `workspace_access(role='owner')` row를 만든다.
- Workspace creator의 owner row는 일반 access API로 revoke하거나 editor/viewer로 downgrade할 수 없다.
- 마지막 active user owner는 revoke/downgrade할 수 없다.
- Agent account는 owner role을 받을 수 없다.
- Owner row를 제거해야 하는 예외는 workspace delete 또는 user account deletion lifecycle뿐이다.
- 위반 요청은 `409 conflict`로 거부한다.

## Invariant 방어 정책

### Prevent

DB는 다음 구조적 invariant를 constraint/index로 막는다.

```text
FK 참조 무결성
workspace당 root node 최대 1개
live sibling name unique
role enum 제한
soft delete timestamp/deleted_by/purge_after 조합
agent key token_hash unique
```

### Coordinate

Service layer는 다음 정책을 transaction으로 보장한다.

```text
user 최초 생성 identity row
workspace 생성 + root + owner access
workspace/access/agent/key count limit
owner revoke/downgrade 보호
workspace/user/agent 삭제 side effect
```

### Detect / contain

런타임에서 깨진 상태를 만나면 추측으로 권한을 보정하지 않는다.

예:

```text
live workspace인데 active user owner가 없음
workspace root node가 없음
role 값이 알 수 없음
document row와 node row 관계가 깨짐
```

이 경우 권한은 fail closed로 처리하고, 사용자에게는 적절한 not-found/forbidden/internal error를 반환하며 운영 로그에 invariant violation을 남긴다.

### Repair

복구는 hot path에서 자동 수행하지 않고 admin repair path에서 처리한다.

```text
root 없는 workspace     -> 명확하면 root 재생성, 아니면 quarantine/soft delete
owner 없는 workspace    -> 명확한 active creator가 있으면 owner row 복구, 아니면 soft delete/quarantine
inactive agent key      -> revoke
inactive account access -> revoke
```
