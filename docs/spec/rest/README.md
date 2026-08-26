# V1 브라우저 API

V1 REST는 NoteGate 브라우저 UI 전용 resource API다. UI가 선택한 `space_id`, `node_id`를 사용해 rename/move 이후에도 선택 상태를 안정적으로 유지한다. 외부 확장은 `../public-api-v2.md`의 V2 계약을 사용한다.

## Categories

| Category | Scope | Path | Doc |
|---|---:|---|---|
| Identity | global | `/api/v1/me`, `/api/v1/me/usage` | `identity.md` |
| Events | global/space | `/api/v1/me/audit-events`, `/api/v1/me/command-invocations`, `/api/v1/me/jobs`, `/api/v1/spaces/{space_id}/file-change-events`, `/api/v1/spaces/{space_id}/file-change-sync` | `events.md` |
| Spaces | global | `/api/v1/spaces`, `/api/v1/spaces:reorder`, `/api/v1/spaces/{space_id}/actions/reconcile-usage` | `spaces.md` |
| Agents | global | `/api/v1/agents` | `agents.md` |
| Connections | space | `/api/v1/spaces/{space_id}/agents` | `connections.md` |
| Nodes | space | `/api/v1/spaces/{space_id}/nodes` | `nodes.md` |
| Links | space | `/api/v1/spaces/{space_id}/nodes/{node_id}/links`, `/api/v1/spaces/{space_id}/actions/reindex-links`, `/api/v1/spaces/{space_id}/link-index` | `links.md` |
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
{
  "result": "accepted",
  "availability": {"can_trigger": false, "reason": "pending", "retry_at": null}
}
```

- `result`는 새 명령을 접수하면 `accepted`, 같은 범위의 명령이 이미 진행 중이면
  `already_pending`이다. 두 경우 모두 요청한 최종 상태가 이미 보장되므로 `202`다.
- `availability`는 같은 명령을 다시 실행할 수 있는지를 나타낸다. `pending`과 `cooldown`은
  각각 진행 중인 명령과 서버 cooldown을 뜻하며 cooldown은 `retry_at`을 제공한다.
- 응답의 `Location`은 명령 상태를 소유한 domain resource를 가리킨다. Background job ID는
  실행 세부사항이므로 command API에서 노출하지 않는다.
- cooldown처럼 현재 요청을 받아들일 수 없는 상태는 공통 `409` error 계약을 사용한다.
- 실행 상태와 `availability`는 Usage, Link index, Node links처럼 해당 상태를 소유한
  domain resource에 포함한다. Job History는 별도 관찰 surface이며 버튼 상태에 사용하지 않는다.

## Permission summary

```text
user owns space       -> read/write/manage
agent connection read -> read/list/stat
agent connection write -> read + create/update/move/delete
```
