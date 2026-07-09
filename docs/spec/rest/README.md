# REST API

REST는 브라우저/UI용 resource API다. UI가 선택한 `space_id`, `node_id`를 사용해 rename/move 이후에도 선택 상태를 안정적으로 유지한다.

## Categories

| Category | Scope | Path | Doc |
|---|---:|---|---|
| Identity | global | `/api/v1/me` | `identity.md` |
| Spaces | global | `/api/v1/spaces` | `spaces.md` |
| Agents | global | `/api/v1/agents` | `agents.md` |
| Connections | space | `/api/v1/spaces/{space_id}/agents` | `connections.md` |
| Nodes | space | `/api/v1/spaces/{space_id}/nodes` | `nodes.md` |
| Text | space | `/api/v1/spaces/{space_id}/text` | `text.md` |
| Files | space | `/api/v1/spaces/{space_id}/files` | `files.md` |

## Auth mapping

```text
browser/authgate bearer -> user account
ngk_v1_ user key       -> user account
ngk_v1_ agent key      -> agent account
```

## Permission summary

```text
user owns space       -> read/write/manage
agent connection read -> read/list/stat
agent connection write -> read + create/update/move/delete
```
