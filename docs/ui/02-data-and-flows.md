# UI 데이터와 흐름

## Backend 자원

| 자원 | UI 위치 | 주요 API |
|---|---|---|
| me/session | Auth, Settings | `GET /api/v1/me` |
| spaces | ActivityRail | `GET/POST/PATCH/DELETE /api/v1/spaces` |
| nodes | Files, Recent, Editor, Inspector | `/api/v1/spaces/{space_id}/nodes...` |
| text | EditorArea | `/api/v1/spaces/{space_id}/text/{node_id}` |
| files | EditorArea | `/api/v1/spaces/{space_id}/files/{node_id}` |
| metadata | Inspector | `/api/v1/spaces/{space_id}/nodes/{node_id}/metadata` |
| agents | Settings Agents | `/api/v1/agents` |
| api keys | Settings Account/Agents | `/api/v1/me/keys`, `/api/v1/agents/{id}/keys` |
| connections | Settings Agents | `/api/v1/spaces/{space_id}/agents` |

## 상태 분류

| 상태 | 소유자 | 저장 |
|---|---|---|
| 서버 자원 | React Query | cache only |
| active space id | UI store | local storage 가능 |
| editor groups | UI store | session only |
| opened node snapshot | UI store | session only |
| sidebar size/visibility | UI store | local storage 가능 |
| theme | UI store | local storage 가능 |
| text draft | draft/component state | session only |
| hover/menu/drag/scroll | component state | 저장 안 함 |

규칙:

- 서버 collection은 UI store에 복제하지 않는다.
- EditorGroup은 현재 열린 node snapshot만 보관할 수 있다.
- text body와 file content는 UI store에 보관하지 않는다.
- cursor와 scroll position은 reload 후 복원하지 않는다.

## Auth

```text
App load
-> GET /api/v1/me
-> success: AppShell
-> 401: AuthScreen
```

```text
Logout
-> POST /auth/logout
-> clear local API key fallback
-> reset session
-> AuthScreen
```

```text
any /api/v1/* returns 401
-> clear local API key fallback
-> reset session
-> AuthScreen
```

Browser session refresh는 server-side flow다. FE는 refresh token을 저장하거나 직접 refresh endpoint를 호출하지 않는다. `/api/v1/me` 401은 재로그인 필요 상태로 처리하고, 503 `auth_unavailable`은 세션을 지우지 않는 일시 장애/재시도 상태로 처리한다.

## Space

### ActivityRail

표시:

- space initials.
- selected state.
- add-space button.
- settings button.

규칙:

- space 정렬은 `sort_order` 기준.
- drag reorder는 `PATCH /spaces/{id}`로 저장한다.
- account/settings는 SettingsModal에 둔다.

### Select

```text
click space
-> set activeSpaceId
-> persist lastActiveSpaceId
-> reset editor groups
-> close mobile sheets
```

### Create

```text
SpaceAddButton
-> dialog
-> POST /api/v1/spaces
-> refresh spaces
-> select created space
```

### Reorder

```text
drag space
-> show drop indicator
-> compute sort_order
-> PATCH changed spaces
-> refresh spaces
```

### Delete

```text
explicit delete
-> confirm
-> DELETE /api/v1/spaces/{space_id}
-> refresh spaces
-> clear related editor groups
```

## PrimarySidebar

### FilesSection

데이터:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
```

규칙:

- root `/`는 보이지 않는다.
- folder row click은 folder node를 열고 expand/collapse도 수행한다.
- text/file row click은 active EditorGroup에 연다.
- drag/drop은 node를 folder 안으로 이동한다.
- sibling manual reorder는 하지 않는다.
- root/empty/folder context에서 create/upload를 제공한다.

### RecentSection

데이터:

```text
GET /api/v1/spaces/{space_id}/nodes?sort=updated_at_desc&limit=...&cursor=...
```

규칙:

- Recent는 항상 PrimarySidebar에 있다.
- generic node-list API를 사용한다.
- row 선택 시 node를 열고 Files reveal을 시도한다.
- reveal 실패는 open을 막지 않는다.

### Load more

```text
scroll near end
-> fetch next cursor page
-> append visible rows
```

## Node actions

### Create

```text
folder/text/file create
-> choose parent folder
-> POST node/text/file API
-> refresh affected children/recent
-> open created node when applicable
```

### Rename

```text
rename
-> PATCH /nodes/{node_id}
-> refresh node, children, recent
-> update opened node snapshot
```

### Move

```text
move into folder
-> POST /nodes/{node_id}/move
-> refresh old/new parents, reveal, recent
-> update opened node snapshot
```

### Delete

```text
delete
-> confirm
-> DELETE /nodes/{node_id}
-> refresh children/recent
-> clear opened editor group if deleted node was open
```

## EditorArea

node kind별 데이터:

```text
folder -> node detail
text   -> node detail + text content
file   -> node detail + file metadata/download
```

규칙:

- header에는 node name만 표시한다.
- path와 metrics는 Inspector에 둔다.
- text preview가 기본이다.
- plain text는 단순 메모처럼 보여준다.
- markdown은 GFM, code highlight, Mermaid를 지원한다.
- markdown preview는 leading YAML frontmatter object를 Obsidian-style Properties로 표시하고 raw YAML block은 본문 prose로 렌더링하지 않는다.
- markdown frontmatter는 Text content이며 Inspector metadata와 동기화하지 않는다.
- JSON/JSONL/YAML/TOML은 Tree/Source view를 제공한다.
- structured tree는 기본 expanded 상태다.
- edit mode는 line number를 보여준다.

### Open

```text
open node
-> set active EditorGroup node snapshot
-> fetch detail/content by kind
-> show Inspector for active node
```

### Split

```text
split
-> if group count < 3: add group to the right
-> new group starts with current active node or empty state
```

### Save text

```text
edit text
-> PUT /text/{node_id} with expected_sha256
-> success: preview mode + refresh node/text
-> conflict: show conflict state
```

### External sync

```text
visible tab polling
-> opened node changed: refresh snapshot
-> text hash changed: refetch text
-> opened node 404: clear editor group
```

## Structured preview

```text
Tree/Source toggle
-> change preview mode only
```

```text
Expand all / Collapse all
-> applies only in Tree mode
```

## Inspector

표시:

- kind, name, path, node id.
- created/updated attribution.
- byte/line metrics.
- metadata JSON.
- metadata privacy note.

규칙:

- 선택 node가 없어도 빈 Inspector를 렌더링한다.
- metadata는 encrypted content가 아니다.
- metadata 수정은 명시 액션으로만 한다.

## Settings

Tabs:

```text
Account | Agents | MCP
```

Account:

- current user/account.
- theme.
- user API keys.
- sign out.

Agents:

- agent list.
- 한 번에 하나의 agent만 펼친다.
- 펼친 agent 안에 agent API keys와 space access를 둔다.

MCP:

- MCP server URL.
- `Authorization: Bearer <credential>` header.
- OAuth, user API key, agent API key 사용 요약.

규칙:

- user API key는 Account에 둔다.
- agent API key는 해당 agent 아래에 둔다.
- connections는 펼친 agent 안에서 관리한다.
- `scopes`는 현재 정책상 표시하지 않는다.

## Context menus

규칙:

- 우클릭은 shortcut이다.
- 같은 action은 버튼, overflow, dialog, touch fallback 중 하나로도 가능해야 한다.
- text editing 영역에서는 native context menu를 막지 않는다.
- destructive action은 confirm이 필요하다.
- touch는 long-press 또는 visible overflow를 사용한다.

| Surface | Target | Actions |
|---|---|---|
| ActivityRail | space | select, rename, delete, copy id |
| Files | empty/root | new folder, new text, upload file |
| Files | folder | open/toggle, create child, upload, rename, move, copy path, delete |
| Files | text | open, open in new group, rename, move, copy path, delete |
| Files | file | open, open in new group, download, rename, move, copy path, delete |
| EditorHeader | node | rename, move, delete, download if file |
| Inspector | metadata | edit metadata |
