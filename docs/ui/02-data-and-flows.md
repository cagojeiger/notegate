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
| active space id | UI store | local storage |
| editor groups, active group, mode | UI store | space별 local storage snapshot |
| opened node snapshot | UI store | space별 local storage snapshot |
| primary/aux sidebar visibility | UI store | local storage |
| primary sidebar width | UI store | session only |
| Files/Recent ratio, section open, density | UI store | session only |
| expanded folders | UI/component state | session only |
| theme | UI store | local storage |
| text draft | draft/component state | session only |
| hover/menu/drag/scroll | component state | 저장 안 함 |

규칙:

- 서버 collection은 UI store에 복제하지 않는다.
- EditorGroup은 열린 pane 복원을 위해 현재 열린 node snapshot만 보관할 수 있다.
- text body와 file content는 UI store에 보관하지 않는다.
- space별 workbench snapshot은 browser-local best-effort 상태다. 계정/서버 정본이 아니며 다른 브라우저로 동기화하지 않는다.
- workbench snapshot은 최근 20개 space까지만 유지한다. 손상됐거나 현재 space와 맞지 않는 snapshot은 폐기한다.
- space 전환 시 현재 space snapshot을 저장하고, 선택한 space snapshot이 있으면 복원한다. 없으면 빈 editor group으로 시작한다.
- Settings의 Saved workspace reset은 browser에 저장된 pane snapshot과 panel visibility만 지운다. note, file, space, 서버 자원은 삭제하지 않는다.
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
-> persist previous space workbench snapshot
-> set activeSpaceId
-> restore selected space workbench snapshot or empty editor groups
-> persist lastActiveSpaceId
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
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?limit=100&cursor=...
GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
```

규칙:

- root `/`는 보이지 않는다.
- folder row click은 folder node를 열고 expand/collapse도 수행한다.
- text/file row click은 active EditorGroup에 연다.
- drag/drop은 node를 folder 안으로 이동한다.
- sibling manual reorder는 하지 않는다.
- root/empty/folder context에서 create/upload를 제공한다.

### Files load more

```text
expand root/folder
-> fetch first children page for that folder
-> scroll near folder page end
-> fetch next cursor page for that folder
-> append visible child rows
```

규칙:

- children pagination은 folder별로 독립적이다.
- root와 각 expanded folder는 같은 children API cursor를 사용한다.
- 자동 load-more는 visible sentinel이 viewport 근처에 들어올 때 수행한다.

### RecentSection

데이터:

```text
GET /api/v1/spaces/{space_id}/nodes?sort=updated_at_desc&limit=50
```

규칙:

- Recent는 항상 PrimarySidebar에 있다.
- generic node-list API를 사용한다.
- 현재 UI는 첫 page만 표시한다.
- row 선택 시 node를 열고 Files reveal을 시도한다.
- reveal 실패는 open을 막지 않는다.

## Node actions

### Create

```text
folder/text create
-> choose parent folder
-> POST node/text API
-> refresh affected children/recent
-> open created node when applicable
```

### Upload file

```text
select file
-> confirm node name
-> POST /file-uploads
-> single: PUT all bytes to the presigned URL
-> multipart: request part URLs, PUT at most 4 parts concurrently
-> POST /file-uploads/{upload_id}/complete with multipart ETags
-> refresh the source space
```

규칙:

- upload는 앱 범위의 memory queue에서 최대 2개 파일까지 실행하므로 space나 node를 이동해도 계속된다. Multipart는 파일당 최대 4개 part를 병렬 전송한다.
- 새로고침이나 tab 종료 뒤에는 이어서 전송하지 않는다. 완료되지 않은 object 정리는 backend 정책을 따른다.
- 100MiB 초과 파일은 64MiB part로 나누고 URL은 16개씩 발급받는다. 실패한 part만 새 URL로 최대 3회 전송한다.
- 취소하거나 최종 실패하면 backend에 upload 정리를 요청한다. 요청 실패 시 backend의 inactivity cleanup이 처리한다.
- 진행 중이거나 실패한 항목은 전역 UploadProgressDock에서 확인한다. 시작 시 대상 space와 folder path를 snapshot으로 보관한다.
- 실패한 항목은 처음부터 재시도하거나 목록에서 제거할 수 있다.
- 완료 항목은 잠시 표시한 뒤 제거한다. 완료 기록의 정본은 Changes event다.
- 완료 시 현재 editor를 file node로 이동하지 않는다.

### Download file

- 파일 다운로드는 브라우저 기본 다운로드 관리자를 사용한다.

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
visible tab: poll latest active-space change event
-> event id changed: invalidate active-space resource cache
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
General | Account | Agents | MCP
```

General:

- saved workspace reset.

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
- Agents tab은 agent 관리 권한이 있는 caller에게만 표시한다.

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
