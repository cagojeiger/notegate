# V1 브라우저 API

V1 REST는 NoteGate 브라우저 UI 전용 resource API다. UI가 선택한 `space_id`, `node_id`를 사용해 rename/move 이후에도 선택 상태를 안정적으로 유지한다. 외부 확장은 `../public-api-v2.md`의 V2 계약을 사용한다.

## Categories

| Category | Scope | Path | Doc |
|---|---:|---|---|
| Identity | global | `/api/v1/me`, `/api/v1/me/usage` | `identity.md` |
| Events | global/space | `/api/v1/me/audit-events`, `/api/v1/me/mcp-invocations`, `/api/v1/spaces/{space_id}/file-change-events`, `/api/v1/spaces/{space_id}/file-change-sync` | `events.md` |
| Spaces | global | `/api/v1/spaces`, `/api/v1/spaces:reorder`, `/api/v1/spaces/{space_id}/usage/reconcile` | `spaces.md` |
| Agents | global | `/api/v1/agents` | `agents.md` |
| Connections | space | `/api/v1/spaces/{space_id}/agents` | `connections.md` |
| Nodes | space | `/api/v1/spaces/{space_id}/nodes` | `nodes.md` |
| Text | space | `/api/v1/spaces/{space_id}/text` | `text.md` |
| Files | space | `/api/v1/spaces/{space_id}/files`, `/api/v1/spaces/{space_id}/file-previews:batchResolve` | `files.md` |

## Auth mapping

```text
browser session cookie -> user account
```

V1은 API key와 OAuth bearer를 허용하지 않는다.

## Permission summary

```text
user owns space       -> read/write/manage
agent connection read -> read/list/stat
agent connection write -> read + create/update/move/delete
```
