# UI implementation plan

이 문서는 notegate web dashboard 구현 계획이다. 구현은 `docs/ui/01-glossary.md`부터 `docs/ui/06-visual.md`까지의 결정을 따른다.

## 1. Target result

초기 목표는 backend REST API를 사용하는 desktop-first dashboard다.

```text
frontend/web
├─ Vite
├─ React
├─ TypeScript
├─ Tailwind CSS
├─ TanStack Query
└─ local UI state store
```

완료 기준:

1. 로그인 이후 `AppShell`이 뜬다.
2. Space 목록을 보고 active space를 선택할 수 있다.
3. Tree/Recent에서 node를 열 수 있다.
4. Text는 preview 기본, edit 전환 후 저장할 수 있다.
5. File은 metadata와 download action을 볼 수 있다.
6. Inspector에서 node property와 metadata를 볼 수 있다.
7. 화면 분할, sidebar resize, tree/recent resize는 local UI state로 동작한다.

## 2. Non-goals for first implementation

초기 구현에서 하지 않는다.

```text
semantic search
agent runtime UI
collaboration/realtime editing
file preview for every media type
mobile polished UX
custom command palette
full offline mode
```

Mobile은 깨지지 않는 수준의 단일-column fallback만 둔다.

## 3. Project scaffold

생성 위치:

```text
frontend/web
```

기본 파일:

```text
frontend/web/
├─ package.json
├─ index.html
├─ vite.config.ts
├─ tsconfig.json
├─ tailwind.config.ts
├─ postcss.config.js
└─ src/
   ├─ main.tsx
   ├─ app/
   ├─ api/
   ├─ components/
   ├─ features/
   ├─ state/
   └─ styles/
```

Root에는 package workspace만 추가한다.

```text
pnpm-workspace.yaml
package.json        # optional root scripts only
```

## 4. Library choices

| 영역 | 선택 | 이유 |
|---|---|---|
| Build | Vite | React SPA 기본값, 빠른 dev server |
| UI | React + TypeScript | 상태와 REST 화면 구현에 적합 |
| Styling | Tailwind CSS + CSS variables | `06-visual.md` token을 직접 매핑 |
| Server cache | TanStack Query | REST server state와 pagination 처리 |
| UI state | Zustand 또는 React state | layout/editor group 같은 local state |
| Icons | lucide-react | 가볍고 workbench형 UI에 충분 |
| Unit test | Vitest | Vite와 동일 생태계 |
| E2E | Playwright | dashboard 흐름 검증 |

초기에는 component library를 도입하지 않는다. Dialog, ContextMenu, Tabs가 복잡해지면 부분적으로 Radix 또는 shadcn/ui 도입을 검토한다.

## 5. State architecture

상태는 네 종류로 나눈다.

```text
Server state    TanStack Query
UI state        local store / React state
Draft state     editor feature state
Ephemeral state component state
```

규칙:

1. Backend resource 객체를 UI store에 복사하지 않는다.
2. UI store에는 id, layout 값, editor group 배열만 둔다.
3. 객체 본문은 query cache에서 읽는다.
4. Mutation 성공 후 관련 query를 invalidate 또는 patch한다.
5. `lastActiveSpaceId`만 local storage에 저장한다.
6. active node는 별도 전역 상태가 아니라 active editor group의 `nodeId`다.

초기 UI store shape:

```ts
type WorkbenchUiState = {
  lastActiveSpaceId: string | null;
  activeEditorGroupId: string | null;
  editorGroups: EditorGroupState[];
  primarySidebarVisible: boolean;
  primarySidebarWidth: number;
  treeRecentRatio: number; // default 2/3 tree, 1/3 recent
  auxiliarySidebarVisible: boolean;
  auxiliaryActiveView: "inspector" | "agent";
};
```

## 6. API client layer

REST 호출은 `src/api`에 모은다.

```text
src/api/
├─ client.ts
├─ spaces.ts
├─ nodes.ts
├─ text.ts
├─ files.ts
└─ metadata.ts
```

필요한 endpoint:

```text
GET    /api/v1/me
GET    /api/v1/spaces
POST   /api/v1/spaces
PATCH  /api/v1/spaces/{space_id}
DELETE /api/v1/spaces/{space_id}
GET    /api/v1/spaces/{space_id}/nodes
GET    /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/children
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/reveal
POST   /api/v1/spaces/{space_id}/nodes
PATCH  /api/v1/spaces/{space_id}/nodes/{node_id}
DELETE /api/v1/spaces/{space_id}/nodes/{node_id}
GET    /api/v1/spaces/{space_id}/text/{node_id}
PUT    /api/v1/spaces/{space_id}/text/{node_id}
PATCH  /api/v1/spaces/{space_id}/text/{node_id}
GET    /api/v1/spaces/{space_id}/files/{node_id}
GET    /api/v1/spaces/{space_id}/files/{node_id}/content
GET    /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PUT    /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PATCH  /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
```

API client 규칙:

1. 모든 요청은 same-origin `/api/v1/...` 기준이다.
2. 개발 서버에서는 Vite proxy가 backend `:9191`로 전달한다.
3. API error는 UI에서 표시 가능한 `message`, `kind`, `status`로 정규화한다.
4. Cursor는 해석하지 않고 다음 요청에 그대로 전달한다.

## 7. Component tree

초기 component 구조:

```text
src/components/layout/
├─ AppShell.tsx
├─ TitleBar.tsx
├─ Workbench.tsx
├─ ActivityRail.tsx
├─ PrimarySidebar.tsx
├─ EditorArea.tsx
├─ AuxiliarySidebar.tsx
└─ StatusBar.tsx

src/components/common/
├─ Button.tsx
├─ IconButton.tsx
├─ Dialog.tsx
├─ ContextMenu.tsx
├─ Tabs.tsx
├─ ResizeHandle.tsx
└─ StatusIndicator.tsx
```

Feature 구조:

```text
src/features/spaces/
src/features/tree/
src/features/recent/
src/features/editor/
src/features/inspector/
src/features/settings/
```

규칙:

- layout component는 위치와 크기만 책임진다.
- feature component는 데이터를 읽고 action을 연결한다.
- common component는 product data를 모른다.

## 8. Implementation phases

### Phase 1. Web scaffold and theme

작업:

1. `frontend/web` 생성.
2. Vite React TS 설정.
3. Tailwind 설정.
4. `06-visual.md` 기반 CSS variables 추가.
5. 빈 `AppRoot`, `AuthScreen`, `AppShell` 렌더링.

검증:

```text
pnpm --filter web build
pnpm --filter web test
```

Acceptance:

- dev server가 열린다.
- dark workbench shell이 보인다.
- Tailwind token class가 동작한다.

### Phase 2. REST client and query foundation

작업:

1. `api/client.ts` 작성.
2. `me`, `spaces`, `nodes`, `text`, `files`, `metadata` client 작성.
3. TanStack Query provider 추가.
4. API error normalization 추가.

검증:

- mocked fetch unit test.
- 실제 backend가 있으면 `/api/v1/me`, `/api/v1/spaces` smoke.

Acceptance:

- API client가 cursor와 error를 안정적으로 다룬다.
- UI component가 직접 `fetch`를 호출하지 않는다.

### Phase 3. AppShell layout

작업:

1. `TitleBar`, `ActivityRail`, `PrimarySidebar`, `EditorArea`, `AuxiliarySidebar`, `StatusBar` 구현.
2. Sidebar resize, tree/recent resize 구현.
3. Editor group split 최대 3개 구현.
4. local storage에 `lastActiveSpaceId` 저장.

검증:

- layout unit test 또는 component test.
- Playwright로 sidebar resize, split add/close smoke.

Acceptance:

- 02-layout.md의 구조와 이름이 코드에 그대로 반영된다.
- node metadata가 header/status에 중복 표시되지 않는다.

### Phase 4. Space and tree browsing

작업:

1. Space rail 목록 로딩.
2. Space create dialog.
3. Tree root/children lazy load.
4. Folder expand/collapse.
5. `reveal` API로 active node tree 위치 동기화.
6. Tree context menu: folder 생성, text 생성, file upload 진입점.

검증:

- children pagination 테스트.
- reveal flow 테스트.
- create folder/text 후 tree 갱신 테스트.

Acceptance:

- 많은 child가 있어도 visible page만 로드한다.
- Recent에서 연 node도 tree에서 reveal된다.
- Tree collapse/expand는 reload 후 복원하지 않는다.

### Phase 5. Recent list

작업:

1. `GET /nodes?sort=updated_at_desc`로 recent list 구현.
2. kind filter 적용.
3. infinite scroll 또는 load-more cursor 처리.
4. RecentSection 독립 scroll 구현.

검증:

- cursor pagination 테스트.
- node update 후 recent invalidation 테스트.

Acceptance:

- Tree와 Recent는 같은 node를 열지만 서로 scroll state를 공유하지 않는다.
- Recent endpoint cursor를 local storage에 저장하지 않는다.

### Phase 6. Editor preview/read

작업:

1. node kind별 viewport 구현.
2. Text preview mode 기본.
3. File metadata view와 download action.
4. Folder selected view.
5. InspectorPanel node property 표시.

검증:

- text/file/folder 각각 열기 테스트.
- encrypted text는 plaintext를 표시하지 않는지 확인.

Acceptance:

- Text body는 필요할 때만 로드한다.
- File content는 자동 다운로드하지 않는다.
- Inspector가 metadata 소유권을 가진다.

### Phase 7. Text edit/save

작업:

1. preview/edit toggle.
2. Draft state 구현.
3. 저장 시 `content_sha256` 기반 conflict 처리.
4. format validation error 표시.
5. 저장 성공 후 draft 제거와 query 갱신.

검증:

- dirty state.
- stale hash conflict.
- JSON/YAML/TOML validation 실패 표시.

Acceptance:

- 사용자가 명시적으로 edit mode에 들어가기 전에는 저장 action이 없다.
- conflict 시 자동 overwrite하지 않는다.

### Phase 8. Metadata and file upload

작업:

1. Inspector metadata view/edit.
2. metadata PUT/PATCH.
3. Inline file upload 256 KiB 제한 표시.
4. upload progress와 error 표시.

검증:

- metadata update 후 모든 영역 반영.
- 256 KiB 초과 upload error 표시.

Acceptance:

- metadata는 content가 아니라 별도 정보로 취급한다.
- file upload는 REST API 제한을 UI에서 사전 안내한다.

### Phase 9. Polish and accessibility baseline

작업:

1. Keyboard navigation: rail, tree, tabs, dialog.
2. Focus ring.
3. Empty/loading/error states.
4. Reduced motion.
5. Basic responsive fallback.

검증:

- Playwright keyboard smoke.
- axe 또는 수동 accessibility checklist.

Acceptance:

- 마우스 없이 기본 탐색과 저장이 가능하다.
- mobile width에서 주요 화면이 깨지지 않는다.

## 9. Verification gates

각 phase는 다음을 통과해야 한다.

```text
pnpm --filter web lint
pnpm --filter web typecheck
pnpm --filter web test
pnpm --filter web build
```

REST 연동 phase 이후:

```text
backend running on :9191
pnpm --filter web e2e
```

PR 전 최소 gate:

```text
cargo test --workspace
pnpm --filter web build
pnpm --filter web test
pnpm --filter web e2e
```

## 10. Risks and mitigations

| 위험 | 대응 |
|---|---|
| UI store가 server data를 복제해 drift 발생 | store에는 id/layout만 저장한다 |
| Tree가 큰 space에서 느려짐 | children cursor와 Recent list cursor만 사용한다 |
| Editor group과 tree selection이 꼬임 | active node는 active editor group에서 계산한다 |
| Metadata가 여러 영역에 중복 표시됨 | Inspector를 정본 표시 영역으로 둔다 |
| File upload 한도가 사용 중 뒤늦게 드러남 | UI에서 256 KiB 현재 제한을 먼저 표시한다 |
| 디자인 값이 컴포넌트에 흩어짐 | Tailwind token/CSS variable만 사용한다 |

## 11. Stop condition

초기 dashboard 구현은 다음 상태에서 멈춘다.

1. 로그인 이후 workbench가 동작한다.
2. Space, Tree, Recent, Editor, Inspector가 REST API로 연결된다.
3. Text preview/edit/save와 File metadata/download가 가능하다.
4. 핵심 layout state가 local UI state로 동작한다.
5. 자동화된 build/test/e2e gate가 통과한다.

그 다음 작업은 visual polish, mobile UX, command palette, agent UI를 별도 계획으로 분리한다.
