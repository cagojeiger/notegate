# ADR 0005: Audit log와 content event log 분리

## Context

notegate는 보안 리뷰, 사고 조사, 향후 usage/activity 기능을 위해 durable한 작업 이력이 필요하다. 기존 테이블은 현재 상태와 `created_at`, `updated_at`, `deleted_at`, `revoked_at`, `disconnected_at` 같은 lifecycle marker를 보존하지만 append-only operation history를 제공하지 않는다.

로그 도메인에는 서로 다른 통제 목적이 있다.

```text
security/permission management history
content/file-tree operation history
```

두 영역을 하나의 stream에 섞으면 저빈도 compliance evidence와 고빈도 product activity가 섞인다. 그러면 retention, 접근 제어, payload 정책, 향후 replay/projection을 분리해서 판단하기 어려워진다.

## Decision

두 append-only event stream을 사용한다.

```text
audit_events   = account, credential, permission, agent, space 관리 변경
content_events = file-tree와 content domain 변경
```

두 stream 모두 현재 product state의 source of truth가 아니다. Source of truth는 `accounts`, `users`, `agents`, `api_keys`, `spaces`, `space_agent_connections`, `nodes`, `text_objects`, `file_objects` 같은 normalized domain table이다.

두 stream은 commit에 성공한 domain mutation만 기록한다. Request logging, access log, security-denial log, content versioning을 대체하지 않는다.

Event row는 append-only다. Product code는 일반 동작에서 event row를 update/delete하지 않는다. 잘못된 row가 생기면 기존 row를 조용히 수정하지 않고 명시적인 repair/reconciliation으로 처리한다.

## Consequences

- Audit review는 text/file edit에 묻히지 않고 access boundary와 credential 변경에 집중할 수 있다.
- Content event는 activity feed, usage projection, replay/reconciliation으로 확장할 수 있다.
- Domain mutation code는 state change와 같은 DB transaction 안에서 event row를 insert해야 한다.
- Event payload는 allowlist 기반이어야 한다. Raw request body, secret, token, text content, file bytes, user PII는 어느 stream에도 저장하지 않는다.
- 두 domain을 모두 건드리는 mutation은 향후 각 stream에 event를 하나씩 남길 수 있다. 초기 설계는 명확한 audit/projection 필요가 없으면 mutation마다 primary stream 하나를 선호한다.
