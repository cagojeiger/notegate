# UI 구현 규칙

## Source tree

```text
frontend/web/src
├─ app/        # entry, auth boundary
├─ api/        # REST client, query keys, API types
├─ auth/       # session/login helpers
├─ design/     # tokens and global theme styles
├─ layout/     # AppShell, frames, Settings, dialogs
├─ features/   # spaces, nodes, editor, workbench behavior
├─ stores/     # UI/draft stores
└─ shared/     # shared UI and utilities
```

## State ownership

| State | Owner |
|---|---|
| `/api/v1/me` | React Query |
| spaces/nodes/text/file/metadata | React Query |
| active space id | UI store |
| editor groups | UI store |
| opened node snapshot | UI store |
| sidebar visibility/size | UI store |
| Files/Recent ratio | UI store |
| theme | UI store |
| expanded folders | UI/component state |
| cursors | query/component state |
| text draft | draft/component state |
| hover/menu/drag | component state |

## Auth boundary

- `useSessionQuery`가 `/api/v1/me`의 authority다.
- `/me` 401은 AuthScreen을 렌더링한다.
- 일반 query/mutation 401은 session reset을 유발한다.
- logout은 local developer API key fallback을 지운다.
- browser session refresh token flow는 없다.

## React Query

- query key는 `api/queryKeys.ts`에 둔다.
- mutation 후 영향 범위 query를 invalidate/refetch한다.
- global mutation error는 toast로 보여준다.
- session query는 중복 401 처리를 피한다.

## External sync

Dashboard sync는 polling + focus/reconnect refetch만 사용한다.
WebSocket과 SSE는 사용하지 않는다.

Polling은 `document.visibilityState === "visible"`일 때만 돈다.

| Query | Interval |
|---|---:|
| opened node freshness | 30s ±5s |
| Recent list | 60s ±10s |
| visible/expanded folder children | 60s ±10s |

규칙:

- opened node freshness는 folder/text/file 모두에 적용한다.
- opened node가 404면 editor group을 비운다.
- text body는 직접 polling하지 않는다.
- text hash가 바뀔 때만 text content를 다시 읽는다.

## Zustand

Zustand가 소유하는 것:

- active space id.
- active editor group.
- editor groups.
- layout visibility/size.
- theme.
- section open/ratio.

Zustand가 소유하지 않는 것:

- node collection.
- text body.
- file content.
- API key secret.

## Visual source

실제 token 정본은 코드다.

```text
frontend/web/src/design/theme.css
```

문서는 role만 고정한다.

| Role | CSS variable |
|---|---|
| background | `--ng-bg` |
| surface | `--ng-surface` |
| editor | `--ng-editor` |
| panel | `--ng-panel` |
| border | `--ng-border` |
| seam | `--ng-seam` |
| selection | `--ng-selection` |
| hover | `--ng-hover` |
| text | `--ng-text` |
| muted | `--ng-muted` |
| faint | `--ng-faint` |
| primary | `--ng-primary` |
| danger/success/warning | `--ng-danger`, `--ng-success`, `--ng-warning` |

## Visual rules

- Light mode는 밝고 따뜻한 neutral surface.
- Dark mode는 graphite 계열.
- 읽기 영역은 가장 깨끗한 surface로 둔다.
- 불필요한 nested card를 만들지 않는다.
- primary color는 selected state와 primary action에만 쓴다.
- hover/focus 시 클릭 가능성이 보여야 한다.
- UI font는 Apple/system sans stack.
- editor/code font는 monospace stack.
- Button/input radius는 8-10px.
- Panel/card radius는 12-16px.
- shadow는 popover/dialog/focus에만 사용한다.

## Area style

| Area | 규칙 |
|---|---|
| TitleBar | 중앙은 비우고 layout/theme controls는 오른쪽에 둔다 |
| ActivityRail | selected space, add-space, settings 위치를 명확히 둔다 |
| PrimarySidebar | source-list density, row border 없음, subtle hover/selected |
| EditorArea | plain text는 메모처럼, markdown/code/mermaid/structured preview 지원 |
| AuxiliarySidebar | 빈 Inspector도 보여주고 metadata warning은 과하게 강조하지 않는다 |

## Tests

필수 확인:

```text
pnpm --filter web typecheck
pnpm --filter web test -- --run
pnpm --filter web build
```

우선 테스트 대상:

- pure helper.
- state reducer.
- auth boundary.
- settings/key manager.
- editor preview/parser.

Playwright smoke는 실제 layout, hover, drag, split pane, browser session이 필요할 때만 사용한다.
