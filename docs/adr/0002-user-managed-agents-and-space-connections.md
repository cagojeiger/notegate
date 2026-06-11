# ADR 0002: User-managed agents and space connections

## Context

notegate의 agent는 user와 동급의 관리 주체가 아니라, user가 만든 worker다. Agent는 독립 credential로 인증되어 작업 attribution을 남기지만, space/agent/key lifecycle을 관리하지 않는다.

## Decision

권한 모델은 user 중심으로 둔다.

```text
User  = control-plane owner; spaces와 agents를 관리한다
Agent = user-managed worker; 연결된 spaces에서만 read/write 한다
```

Space는 user가 소유한다. Agent는 space에 `connection`으로 연결되며 permission은 `read` 또는 `write`다.

```text
user owns spaces
user owns agents
user connects agent to space
agent acts inside connected space
```

## Rules

- Space 생성/이름 변경/삭제는 owner user만 가능하다.
- Agent 생성/삭제/API key 관리는 owner user만 가능하다.
- Agent는 space를 소유하거나 관리하지 못한다.
- Agent connection은 `read` 또는 `write`만 가진다.
- `write`는 `read`를 포함한다.
- 파일/폴더 단위 ACL은 두지 않는다.
- 작업 attribution은 user/agent 모두 `accounts`로 기록한다.

## Consequences

- 제품 모델은 개인 user가 관리하는 agent connection을 중심으로 한다.
- UI 용어는 `connect agent`, `disconnect agent`, `permission`을 사용한다.
- 내부 인증 주체는 user/agent 모두 account지만, lifecycle 관리 주체는 user다.
