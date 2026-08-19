# V1 브라우저 API

V1 REST는 NoteGate 브라우저 UI 전용 resource API다. UI가 선택한 `space_id`, `node_id`를 사용해 rename/move 이후에도 선택 상태를 안정적으로 유지한다. 외부 확장은 `../public-api-v2.md`의 V2 계약을 사용한다.

## Categories

| Category | Scope | Path | Doc |
|---|---:|---|---|
| Identity | global | `/api/v1/me`, `/api/v1/me/usage` | `identity.md` |
| Events | global/space | `/api/v1/me/audit-events`, `/api/v1/me/mcp-invocations`, `/api/v1/me/jobs`, `/api/v1/spaces/{space_id}/file-change-events`, `/api/v1/spaces/{space_id}/file-change-sync` | `events.md` |
| Spaces | global | `/api/v1/spaces`, `/api/v1/spaces:reorder`, `/api/v1/spaces/{space_id}/usage/reconcile` | `spaces.md` |
| Agents | global | `/api/v1/agents` | `agents.md` |
| Connections | space | `/api/v1/spaces/{space_id}/agents` | `connections.md` |
| Nodes | space | `/api/v1/spaces/{space_id}/nodes` | `nodes.md` |
| Links | space | `/api/v1/spaces/{space_id}/nodes/{node_id}/links`, `/api/v1/spaces/{space_id}/link-index/reindex`, `/api/v1/spaces/{space_id}/link-index/status` | `links.md` |
| Text | space | `/api/v1/spaces/{space_id}/text` | `text.md` |
| Files | space | `/api/v1/spaces/{space_id}/files`, `/api/v1/spaces/{space_id}/file-previews:batchResolve` | `files.md` |

## Auth mapping

```text
browser session cookie -> user account
```

V1은 API key와 OAuth bearer를 허용하지 않는다.

## Asynchronous commands

브라우저가 시작하는 비동기 명령은 `202 Accepted`와 같은 응답 envelope를 사용한다.

```json
{"status":"accepted","job_id":"..."}
```

- `status`는 새 작업을 접수하면 `accepted`, 같은 범위의 작업이 이미 진행 중이면
  `already_pending`이다. 두 경우 모두 요청한 최종 상태가 이미 보장되므로 `202`다.
- 단일 background job을 만들면 `job_id`를 반환하고
  `GET /api/v1/me/jobs/{job_id}`로 상세 상태를 조회할 수 있다.
- 여러 job으로 분할되거나 queue 등록 전 대기 상태를 포함하면 `job_id`는 `null`이고,
  작업을 소유한 resource의 상태 응답을 사용한다.
- cooldown처럼 현재 요청을 받아들일 수 없는 상태는 공통 `409` error 계약을 사용한다.
- 상태는 기존 resource 표현에 자연스럽게 포함할 수 있으면 그 응답에 넣고, fan-out
  작업처럼 독립적인 수명주기가 있을 때만 별도 `/status` endpoint를 둔다.

## Permission summary

```text
user owns space       -> read/write/manage
agent connection read -> read/list/stat
agent connection write -> read + create/update/move/delete
```
