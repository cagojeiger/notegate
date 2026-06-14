# UI implementation

This document defines frontend code ownership and state boundaries.

## Source tree

```text
frontend/web/src
├─ app/        # app entry, routes/providers, auth boundary
├─ api/        # REST client, query keys, generated/manual API types
├─ auth/       # browser session and login gate
├─ design/     # tokens and global theme styles
├─ layout/     # AppShell, TitleBar, Workbench frames, Settings, dialogs
├─ features/   # spaces, nodes, editor, workbench behavior
├─ stores/     # client UI/draft stores
└─ shared/     # reusable UI primitives and utilities
```

## State ownership

| State | Owner | Persistence |
|---|---|---|
| `/api/v1/me` | React Query | none |
| spaces/nodes/text/file/metadata | React Query | cache only |
| active space id | UI store | local storage allowed |
| editor groups | UI store | session only |
| sidebar visibility/width | UI store | local storage allowed |
| Files/Recent ratio | UI store | local storage allowed |
| theme | UI store | local storage allowed |
| expanded folders | UI store/component state | session only |
| pagination cursors | query/component state | session only |
| text draft | draft store | session only unless explicitly allowed |
| hover/menu/drag target | component state | none |

## Auth boundary

Rules:

- `App` reads stored developer API key once on boot.
- `useSessionQuery` calls `/api/v1/me` and is the session authority.
- `/me` 401 renders `AuthScreen`.
- Non-session query/mutation 401 resets auth state and returns to `AuthScreen`.
- Logout clears local developer API key fallback and bumps session revision.
- Browser session refresh token flow is not implemented.

## React Query rules

- Query keys live in `api/queryKeys.ts`.
- Server objects are read from query cache; editor groups keep only the currently opened node snapshot for pane rendering.
- Mutations invalidate or refresh affected queries.
- Global mutation errors show toast unless the mutation opts out with `silentError`.
- Auth/session query uses query meta to avoid duplicate global 401 handling.

## External sync

Dashboard sync uses slow polling plus focus/reconnect refetch. It does not use WebSocket or SSE.

Automatic polling runs only while the browser document is visible. Hidden tabs keep their current cache and rely on focus/reconnect refetch when the user returns.

| Query | Default interval |
|---|---:|
| opened node freshness | 30s ±5s |
| Recent list | 60s ±10s |
| visible/expanded folder children | 60s ±10s |

Opened node freshness applies to text, file, and folder nodes. If the opened node no longer exists, the editor group is cleared on the next visible-tab poll.

Text bodies are not polled directly. The dashboard checks node metadata first and refetches text content only when the content hash changes.

## Zustand rules

Zustand owns UI state only:

- active space id
- active editor group id
- editor groups
- layout visibility and sizes
- theme
- section open/ratio state

Zustand does not own:

- node collections beyond opened editor snapshots
- text bodies
- file metadata
- API key secrets returned once

## Component ownership

| Component area | Owns |
|---|---|
| `ActivityRail` | space selection, space reorder interaction |
| `PrimarySidebar` | Files/Recent rendering, section resize, node context menu entry |
| `EditorArea` | editor group rendering and active group presentation |
| `TextPreview` | preview mode selection for markdown/plain/structured text |
| `TextEditor` | text editing presentation and save action wiring |
| `AuxiliarySidebar` | Inspector presentation |
| `SettingsModal` | Account, Agents, and MCP settings surfaces |

## Test baseline

Required checks for UI changes:

```text
pnpm --filter web typecheck
pnpm --filter web test -- --run
pnpm --filter web build
```

Add focused tests for:

- pure formatting/parsing helpers
- state reducers/selectors
- auth boundary behavior
- key component interactions that do not require a live browser

Use Playwright smoke only for flows that require real layout, hover, drag, split panes, or browser session behavior.
