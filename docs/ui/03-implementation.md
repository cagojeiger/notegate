# UI 구현 규칙

## Source tree

```text
frontend/web/src
├─ app/        # entry, auth boundary
├─ api/        # REST client, query keys, API types
├─ auth/       # session/login helpers
├─ design/     # tokens and global theme styles
├─ layout/     # AppShell and responsive frames
├─ features/   # spaces, nodes, editor, settings, dialogs, workbench behavior
├─ stores/     # UI/draft stores
└─ shared/     # shared UI and utilities
```

## State ownership

Server state는 React Query, UI 상태는 UI store, draft와 일시적인 상호작용은 draft/component state가 소유한다. 세부 저장 여부·수명·reset 범위는 [상태 분류](02-data-and-flows.md#상태-분류)가 정본이다.

## Auth boundary

`useSessionQuery`가 `/api/v1/me`의 authority다. Session query는 중복 401 처리를 피한다. 로그인·401/503·server-side refresh 동작은 [Auth](02-data-and-flows.md#auth)를 따른다. Browser V1에 API key를 보내지 않는다.

## React Query

- Query key는 `api/queryKeys.ts`에 둔다.
- Cold tree의 cache 없는 root/expanded folder 첫 page를 최대 16개씩 batch 조회한다. 요청 중 children invalidation이 없었을 때만 folder별 cache에 채운다.
- Children은 mutation/forward sync가 명시적으로 reset하므로 observer mount만으로 재조회하지 않는다.
- 구조 변경은 Recent와 영향받은 parent children의 continuation을 버리고 active observer의 첫 page만 읽는다.
- Folder rename/move/recursive delete는 descendant path가 바뀌므로 해당 Space의 node/children/path cache family를 invalidate한다.
- 공통 updater로 node/Recent/children/path의 같은 Node를 갱신한다. Collection은 `view=summary`와 compact field만, editor store는 canonical node query 결과만 사용한다.
- External sync는 typed parent 범위로 invalidate하고 invalid token만 file-related cache family로 fallback한다. Active Space 전체 invalidation은 수동 refresh에만 쓴다.
- Global mutation error는 toast로 표시한다.

## External sync

Active Space polling + focus/reconnect refetch를 사용하며 WebSocket/SSE는 없다. Polling은 `document.visibilityState === "visible"`에서만 실행한다.

| 상황 | Token / interval |
|---|---|
| 첫 요청 | Latest event ID를 baseline으로 설정, 과거 이력 재생 안 함; 30초 |
| 변경 없음 | 30 → 60 → 120 → 300초 cap, 각 ±5초 |
| 변경/resync/error, 화면 복귀, focus/reconnect, Space 전환 | 30초로 reset |
| Forward sync | 마지막 적용 ID 이후 event를 오름차순으로 모든 page 수신 후 token 전진; 같은 parent invalidation 병합 |
| Token retention 초과 | node/children/text/file/path/preview cache만 한 번 재동기화 |

Opened node·Recent·expanded folder는 개별 polling하지 않는다. Folder/Text/File 모두 freshness를 적용하고 opened node가 404면 group을 비운다. Text body는 직접 polling하지 않고 change event로 query를 invalidate한다.

Markdown image path는 같은 렌더 단계에서 최대 64개 또는 UTF-8 16 KiB로 batch 조회한다. Path별 query cache와 ready URL의 Node별 preview cache를 공유한다. 응답 순서/개수가 계약과 다르면 캐시하지 않는다. 표시·실패 처리는 [Markdown image preview](02-data-and-flows.md#markdown-image-preview)를 따른다.

## Zustand

Active Space/group, editor groups, layout visibility/size, theme, section open/ratio를 소유한다. Node collection, Text body, File content, API key secret은 넣지 않는다. Browser-local 저장은 [State ownership](#state-ownership)의 정본을 따른다.

## Visual source

Token 값의 정본은 `frontend/web/src/design/theme.css`다. 문서는 role만 고정한다.

| Role | CSS variable |
|---|---|
| background / surface / editor / panel | `--ng-bg`, `--ng-surface`, `--ng-editor`, `--ng-panel` |
| border / seam / selection / hover | `--ng-border`, `--ng-seam`, `--ng-selection`, `--ng-hover` |
| text / muted / faint / primary | `--ng-text`, `--ng-muted`, `--ng-faint`, `--ng-primary` |
| danger / success / warning | `--ng-danger`, `--ng-success`, `--ng-warning` |

## Visual rules

Palette, typography, 상태 표시, 접근성은 [DESIGN.md](../../DESIGN.md)가 정본이다.

- Space Library 스크린샷 기준은 Linux CI의 회귀 검사다. OS별 시스템 글꼴 모양의 동일성을 보장하지 않는다.
- Workbench control/row 4px, section surface와 Panel 6px, 독립 입력·중앙 modal 8px, 독립 card 16px radius.
- Shadow는 popover/dialog/focus에만 사용한다.

## Area style

| Area | 규칙 |
|---|---|
| TitleBar | 중앙은 비우고 layout/theme controls는 오른쪽 |
| ActivityRail | Selected space, add-space, settings 위치 명확히 표시 |
| PrimarySidebar | Source-list density, row border 없음, subtle hover/selected |
| EditorArea | Plain text는 메모처럼, markdown frontmatter/code/mermaid/structured preview |
| AuxiliarySidebar | 빈 Inspector도 표시, metadata warning 과도한 강조 금지 |

## Brand assets

| 항목 | 정본·규칙 |
|---|---|
| 자산 | `frontend/web/public/brand/`; wordmark는 host font에 의존하지 않는 SVG path |
| 크기 | 32px 미만 app icon, 이상 symbol/horizontal lockup |
| 출력 | `pnpm --dir frontend/web export:icons`: favicon, Apple touch, PWA, maskable, Windows tile |
| 제품명 | `NoteGate` |
| 로그인 | `Continue with Google`; AuthGate를 사용자 provider로 노출하지 않음 |
| Google G | `frontend/web/public/google-g.png` 공식 배포본 |
| Google CTA | 공식 HTML button configurator의 크기·색상·pill shape·Roboto/Arial stack |

제품·접근성 결정은 [DESIGN.md](../../DESIGN.md)를 따른다.

## Tests

```text
pnpm --filter web typecheck
pnpm --filter web test -- --run
pnpm --filter web build
pnpm --filter web test:e2e
```

화면 변경은 desktop `1440×900`, tablet `900×1024`, mobile `390×844`의 light/dark와 login/reflow의 최소 `320 CSS px`를 확인한다. Pure helper, reducer, auth boundary, settings/key manager, preview/parser를 우선 검증한다. Playwright는 실제 layout·hover·drag·split·browser session이 필요한 경우에 사용한다.
