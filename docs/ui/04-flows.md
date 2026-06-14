# UI flows

This document defines dashboard user flows and the state they change.

## State classes

```text
Server state    Backend-owned resources cached by React Query
UI state        Layout and selection state owned by the client
Draft state     Unsaved editor content
Ephemeral state Hover, menus, loading, cursors, scroll, drag target
```

Rules:

- Backend resources are not copied into UI stores as a second source of truth.
- UI stores keep ids and layout values, not full resource objects.
- Active node is the node opened in the active editor group.
- `lastActiveSpaceId` may be persisted locally.
- Editor groups, folder expansion, cursors, and scroll positions are not restored after reload.

## Auth

### App load

```text
AppRoot
-> GET /api/v1/me
-> success: AppShell
-> 401: AuthScreen
```

### Login

```text
AuthScreen login
-> OAuth popup or API key fallback
-> refetch /api/v1/me
-> success: AppShell
-> failure: AuthScreen with error/progress state
```

### Logout

```text
Settings Account Sign out
-> POST /auth/logout
-> clear local API key fallback
-> reset session revision
-> AuthScreen
```

### Session expired

```text
any /api/v1/* returns 401
-> clear local API key fallback
-> reset session revision
-> AuthScreen
```

Notegate dashboard does not refresh browser sessions with refresh tokens. Expired sessions require login.

## Space flows

### Select space

```text
ActivityRail click space
-> set active space
-> persist lastActiveSpaceId
-> reset Files/Recent pages
-> show empty EditorArea if no node is open for the new space
-> show empty Inspector if no active node
```

### Create space

Available when `/api/v1/me.capabilities.can_create_space` is true.

```text
SpaceAddButton
-> create-space dialog
-> POST /api/v1/spaces
-> refresh spaces
-> select created space
```

### Reorder spaces

Available for callers that can manage spaces.

```text
drag space item
-> show drop position
-> update visible order optimistically
-> PATCH changed sort_order values
-> refresh spaces
```

### Delete space

```text
explicit management action
-> confirm
-> DELETE /api/v1/spaces/{space_id}
-> refresh spaces
-> clear editor groups for deleted space
```

## Navigation flows

### Expand/collapse folder

```text
FilesSection folder click
-> toggle folder expansion
-> if opening and children are missing: GET children page
```

### Select node from Files

```text
FilesSection row click
-> open node in active EditorGroup
-> set active group node id
-> update Files selection
-> update Inspector context
```

### Select node from Recent

```text
RecentSection row click
-> open node in active EditorGroup
-> update Inspector context
-> call reveal for Files when needed
-> reveal success: expand ancestors and select row
-> reveal failure: keep editor open and leave Files partial
```

### Load more

```text
scroll near section end
-> use current cursor
-> fetch next page
-> append visible rows
```

Cursor values are request continuations and are not persisted.

## Context menus

Right-click menus are shortcuts, not the only way to act. Every context-menu action must also be reachable through a visible button, overflow menu, dialog, or touch fallback.

Rules:

- Do not override native browser context menus inside text editing areas or selectable document content.
- Mutating actions are hidden or disabled when the active space is not writable.
- Destructive actions require confirmation.
- Touch devices use long-press or the visible overflow menu.

| Surface | Target | Actions |
| --- | --- | --- |
| ActivityRail | Space item | Select, rename, delete, copy space id. Mutating actions require account capability and writable space. |
| ActivityRail | Add-space button | Open create-space dialog. No right-click menu. |
| PrimarySidebar Files | Empty/root area | New folder, new text, upload file. |
| PrimarySidebar Files | Folder row | Open/toggle, new folder, new text, upload file, rename, move, copy path, delete. |
| PrimarySidebar Files | Text row | Open, open in new group, rename, move, copy path, delete. |
| PrimarySidebar Files | File row | Open, open in new group, download, rename, move, copy path, delete. |
| PrimarySidebar Recent | Any row | Open, reveal in Files, open in new group, copy path, rename, move, delete. |
| Editor group header | Open node | Rename, move, copy path, delete, close group. File nodes also expose download. |
| Editor empty state | Empty group | New text, new folder, upload file. |
| AuxiliarySidebar Inspector | Node card | Copy path, copy node id, edit metadata. |
| Settings | Account/Agents/MCP rows | Use visible row buttons only. No custom right-click menu. |

## Node management flows

### Create folder/text/file

```text
PrimarySidebar header or context menu
-> choose parent: root/empty/folder context
-> POST folder/text or file upload
-> refresh parent children
-> refresh Recent
-> open created text/file when appropriate
```

### Rename

```text
node action
-> prompt new name
-> PATCH node
-> refresh node detail, parent children, Recent
```

### Move

```text
drag/drop onto folder or Move dialog
-> POST move
-> refresh old parent children
-> refresh new parent children
-> reveal moved node
```

Move changes parent/name. It does not manually reorder siblings.

### Delete

```text
node action
-> confirm
-> DELETE node
-> remove from Files/Recent
-> close or clear editor groups showing deleted node
-> clear Inspector if it pointed at deleted node
```

Folder delete requires recursive confirmation.

## Editor flows

### Open node

```text
open node
-> active EditorGroup receives node id
-> fetch node detail
-> fetch text/file data by kind
-> render viewport
```

### Split editor

```text
TitleBar add group
-> add EditorGroup to the right of active group
-> focus new group
-> disable add action when group count is 3
```

### Close group

```text
EditorGroupHeader close
-> remove group
-> choose adjacent group as active
```

### Preview/edit toggle

```text
Edit button
-> switch active group mode
-> preview hidden, editor shown
```

### Save text

```text
TextEditor save
-> PUT/PATCH text with expected sha when available
-> success: clear draft, refresh text/node/Recent
-> conflict: keep draft and show conflict state
```

Drafts are tied to node id and base content hash.

## Structured preview flows

### Tree/source toggle

```text
Structured text file open
-> parse JSON/JSONL/YAML/TOML
-> show Tree by default
-> Source button shows original content
```

### Expand/collapse structured tree

```text
header expand/collapse controls
-> apply to structured viewer only
-> backend state unchanged
```

## Metadata flow

```text
Inspector edit metadata
-> validate JSON object
-> PUT/PATCH metadata
-> refresh node metadata
```

Metadata is searchable/displayable data, not encrypted text content.

## Layout flows

```text
TitleBar controls
-> toggle PrimarySidebar
-> add EditorGroup
-> toggle AuxiliarySidebar
```

```text
PrimarySidebar resize
-> update local width
```

```text
Files/Recent divider drag
-> update local section ratio
```

Layout state is local UI state. It is not sent to the backend.

## Limits surfaced by UI

```text
owned spaces per user       <= 20
space live nodes            <= 25,000
space live content bytes    <= 1 GiB
path depth below root       <= 7
folder direct children      <= 1,000
children page size          default 100, max 200
text object bytes           <= 1 MiB
file object bytes           <= 100 MiB
inline file upload          <= 256 KiB currently supported
editor groups               <= 3 desktop
```

UI should prevent obvious invalid input and still show backend validation errors when returned.
