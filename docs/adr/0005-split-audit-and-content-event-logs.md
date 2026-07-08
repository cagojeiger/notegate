# ADR 0005: Audit log와 content event log 분리

## Context

notegate는 사용자가 자기 계정과 space의 관리 변경을 확인하고 agent owner가 agent의 작업을 되돌아볼 수 있도록 durable한 작업 이력이 필요하다. 기존 테이블은 현재 상태와 `created_at`, `updated_at`, `deleted_at`, `revoked_at`, `disconnected_at` 같은 lifecycle marker를 보존하지만 append-only operation history를 제공하지 않는다.

이력에는 서로 다른 검토 목적이 있다.

```text
account/credential/space 관리 이력
content/file-tree 작업 이력
```

두 영역을 하나의 stream에 섞으면 저빈도 관리 이력과 고빈도 content 활동이 섞인다. 그러면 retention, payload 정책, 조회 방식을 분리해서 판단하기 어려워진다.

## Decision

두 append-only event stream을 사용한다.

```text
audit_events   = account, credential, permission, agent, space 관리 변경
content_events = file-tree와 content domain 변경
```

두 stream 모두 현재 product state의 source of truth가 아니다. Source of truth는 `accounts`, `users`, `agents`, `api_keys`, `spaces`, `space_agent_connections`, `nodes`, `text_objects`, `file_objects` 같은 normalized domain table이다.

두 stream은 commit에 성공한 domain mutation만 기록한다.

Implementation은 audit_events부터 시작하고, content_events는 file-tree activity가 필요해지는 후속 PR에서 추가할 수 있다.

Event row는 append-only다. Product code는 일반 동작에서 event row를 update/delete하지 않는다. 잘못된 row가 생기면 기존 row를 조용히 수정하지 않고 명시적인 repair/reconciliation으로 처리한다.

## Consequences

- 관리 이력 검토는 text/file edit에 묻히지 않고 account와 credential 변경에 집중할 수 있다.
- Content event는 agent 작업 검토와 activity history에 집중할 수 있다.
- Domain mutation code는 state change와 같은 DB transaction 안에서 event row를 insert해야 한다.
- Event payload는 allowlist 기반이어야 한다. Payload 보안 원칙은 `docs/spec/security.md`를 따른다.
- 두 domain을 모두 건드리는 mutation은 향후 각 stream에 event를 하나씩 남길 수 있다. 초기 설계는 명확한 audit 또는 content history 필요가 없으면 mutation마다 primary stream 하나를 선호한다.
