# UI information architecture

이 문서는 layout region이 어떤 정보를 소유하는지 정의한다. 화면 배치는 [`02-layout.md`](./02-layout.md)를 따르고, API shape는 `docs/spec`를 따른다.

## Backend assumptions

대시보드 UI는 REST API를 기준으로 화면을 그린다.

```text
Space list       -> GET /api/v1/spaces
Node children    -> GET /api/v1/spaces/{space_id}/nodes/{node_id}/children
Node detail      -> GET /api/v1/spaces/{space_id}/nodes/{node_id}
Node metadata    -> GET/PUT/PATCH /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
Text content     -> GET/PUT/PATCH /api/v1/spaces/{space_id}/text/{node_id}
File metadata    -> GET /api/v1/spaces/{space_id}/files/{node_id}
File content     -> GET /api/v1/spaces/{space_id}/files/{node_id}/content
```

`RestNode`는 UI 렌더링용 node shape다. Content body는 포함하지 않고, metadata, path, kind, byte_len, updated_at, attribution, text/file summary를 포함한다.

## AuthScreen

`AuthScreen`은 로그인 전용 화면이다.

정보:

- product name
- login action
- login progress
- login success message
- auth error message
- not registered 안내

규칙:

- `AuthScreen`은 `Workbench`를 포함하지 않는다.
- 로그인 이후에는 `AppShell`로 전환한다.

## TitleBar

`TitleBar`는 전역 context와 짧은 action을 표시한다.

정보:

- current space name 또는 product context
- layout controls: PrimarySidebar, add EditorGroup, AuxiliarySidebar
- sync/auth 상태가 필요할 때 짧은 indicator

규칙:

- 긴 목록이나 검색 결과를 표시하지 않는다.
- node tree를 표시하지 않는다.
- 모바일에서는 space 표시를 축약할 수 있다.
- account/profile/settings 진입은 `ActivityRail`의 Settings에서 처리한다.
- editor group 안에 중복 layout/inspector action을 두지 않는다.

## ActivityRail

`ActivityRail`은 Space 전환과 settings 진입을 담당한다.

구성:

```text
ActivityRail
├─ SpaceRailList   # 상단~중간, scrollable
├─ SpaceAddButton  # SpaceRailList 아래, 항상 표시
└─ RailFooter      # 하단 고정
   └─ SettingsButton
```

### SpaceRailList

정보:

- 접근 가능한 space 목록
- active space 표시
- space avatar 또는 짧은 이름
- space 상태 badge가 필요하면 표시

Backend:

```text
GET /api/v1/spaces?limit=...&cursor=...
```

규칙:

- Space가 많으면 `SpaceRailList` 내부만 스크롤한다.
- `ActivityRail` 전체가 page scroll을 만들지 않는다.
- `SpaceAddButton`은 `SpaceRailList` 실제 항목 바로 아래, `RailFooter` 위에 항상 표시한다.
- Settings는 스크롤 영역에 들어가지 않고 하단에 고정한다.
- Space create/delete/rename은 rail 안에서 직접 처리하지 않고 dashboard의 명시적 관리 화면에서 처리한다.

### RailFooter

하단 고정 항목:

```text
settings
```

규칙:

- `settings`는 account와 settings 화면의 진입점이다.
- `ActivityRail`은 파일 트리를 표시하지 않는다.

## PrimarySidebar

`PrimarySidebar`는 active space의 탐색 view를 표시한다.

규칙:

- `PrimarySidebar` 폭은 사용자가 좌우로 조절할 수 있다.

기본 구성:

```text
PrimarySidebar
├─ SidebarHeader
└─ SidebarContent
   ├─ TreeSection
   ├─ SectionResizeHandle
   └─ RecentSection
```

#### TreeSection

`TreeSection`은 Space 안 folder/text/file 계층을 보여준다.

정보:

- folder/text/file name
- kind icon
- selected state
- has_children indicator
- optional byte_len 또는 line_count
- pagination state

Backend:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?limit=...&cursor=...
```

규칙:

- `TreeSection`과 `RecentSection`은 각각 독립적으로 스크롤한다.
- `TreeSection` header는 전체 folder 접기 action을 가질 수 있다.
- `RecentSection` header는 표시 밀도 또는 보기 방식을 바꾸는 action을 가질 수 있다.
- 두 section 사이 divider는 마우스로 높이를 조절할 수 있다.
- 한 번에 전체 tree를 펼쳐서 렌더링하지 않는다.
- 화면에 보이는 folder만 children을 요청한다.
- folder children은 pagination cursor로 추가 로드한다.
- UI는 스크롤 기반 추가 로드를 기본 interaction으로 삼는다.
- backend children page 기본값은 100, 최대 200이다.
- 폴더 하나의 direct children이 많아도 UI 높이는 무한히 커지지 않는다.
- folder row click은 expand/collapse를 토글한다.
- 키보드 위/아래 이동은 현재 보이는 tree/recent item 선택을 이동한다.


### Tree context actions

Folder 또는 root/empty tree context에서 생성 action을 제공한다.

Action:

```text
New Folder
New Text
Upload File
```

규칙:

- Folder context에서 생성하면 해당 folder가 parent가 된다.
- Root/empty context에서 생성하면 active space root가 parent가 된다.
- Text/File context에서는 child 생성 action을 제공하지 않는다.
- 같은 생성 action은 `PrimarySidebar` header에서도 접근 가능해야 한다.
- 생성 후 parent folder children과 recent list만 갱신한다.

#### RecentSection

`RecentSection`은 최근 수정된 node를 단순 list로 보여준다.

정보:

- node name
- path
- kind
- updated_at
- optional updated_by

정렬:

```text
updated_at DESC
```

규칙:

- `Recent`는 tree 구조를 보여주지 않는다.
- 문서/파일을 빠르게 다시 여는 목적이다.
- 현재 backend에 전용 recent endpoint가 없으면 초기 구현에서는 숨기거나, 별도 API가 생긴 뒤 활성화한다.

## EditorArea

`EditorArea`는 현재 열린 node의 주 작업 영역이다.

공통 정보:

- editor groups
- group별 `EditorGroupHeader`
- group header 안의 node name
- group별 `EditorInfoBar`의 path, byte/line count, updated_at
- group별 preview/edit mode
- save state

### EditorGroupHeader

`EditorGroupHeader`는 열린 node의 identity와 group action을 한 줄에 담는다.

정보:

- node icon
- node name
- preview/edit mode action
- group close action

규칙:

- header는 `PrimarySidebar` header, `AuxiliarySidebar` tab header와 같은 기준선에 맞춘다.
- path와 파일 부수 정보는 header가 아니라 group 하단 `EditorInfoBar`에 표시한다.
- node를 새로 선택하면 active `EditorGroup`의 내용을 대체한다. 새 group을 자동으로 계속 추가하지 않는다.
- group을 추가하려면 전역 split/add action을 사용한다.

### EditorInfoBar

`EditorInfoBar`는 각 `EditorGroup` 하단에서 열린 node의 부수 정보를 표시한다. 전역 상태가 아니라 group-local 정보다.

정보:

- path
- kind
- byte_len
- line_count
- updated_at

규칙:

- `TitleBar`와 `StatusBar`는 현재 파일 경로를 반복 표시하지 않는다.
- 파일과 관련된 부수 정보는 `EditorInfoBar`에 모은다.
- `EditorInfoBar`는 content editor가 아니며, 긴 metadata를 표시하지 않는다.
- 여러 `EditorGroup`이 있으면 각 group은 자기 node의 `EditorInfoBar`를 따로 가진다.

### Editor split

`EditorArea`는 VSCode처럼 독립적인 `EditorGroup`을 1개에서 최대 3개까지 표시할 수 있다.

규칙:

- split은 preview/edit 동시 표시가 아니다.
- top-right split control은 새 `EditorGroup`을 오른쪽에 추가한다.
- 각 `EditorGroup`은 서로 다른 node를 열 수 있다.
- 각 `EditorGroup`은 preview/edit mode를 독립적으로 가진다.
- `PrimarySidebar`에서 node를 선택하면 현재 active `EditorGroup`에 열린다.
- `EditorGroup` 제거는 `EditorGroupHeader`의 close action으로 수행한다.
- 최대 group 수에 도달하면 split/add action은 disabled 상태를 보여준다.

### Folder selected

Folder를 선택하면 folder summary 또는 empty state를 보여준다.

정보:

- folder name
- path
- child count summary가 있으면 표시
- create action

### Text selected

Text를 선택하면 기본으로 preview를 보여준다. 사용자가 edit mode로 전환하면 text editor를 보여준다.

정보:

- content
- storage_format: plain 또는 encrypted
- content_sha256
- line_count
- byte_len
- updated_at

규칙:

- plain Text는 기본 preview mode로 읽고, edit mode로 전환해 수정한다.
- encrypted Text는 서버가 복호화하지 않으므로 preview/edit 대신 encrypted 상태를 명확히 표시한다.
- content body는 `RestNode`가 아니라 Text API에서 별도로 가져온다.

### File selected

File을 선택하면 file preview 또는 metadata view를 보여준다.

정보:

- media_type
- original_filename
- byte_len
- content_sha256
- encryption_mode
- download action

규칙:

- File은 Text editor로 열지 않는다.
- preview 가능한 media type만 preview한다.
- content bytes는 File content API에서 별도로 가져온다.

## AuxiliarySidebar

`AuxiliarySidebar`는 선택된 node 또는 editor context의 보조 정보를 표시한다. 지금은 metadata/inspector 중심으로 사용하고, 이후 여러 view를 추가할 수 있다.

초기 view:

```text
InspectorPanel
AgentPanel
```

예약 view:

```text
ReferencesPanel
OutlineView
HistoryPanel
```

### InspectorPanel

선택된 node의 속성과 metadata를 보여준다.

정보:

- node id
- path
- kind
- name
- metadata JSON
- created_by / created_at
- updated_by / updated_at
- text/file summary

Backend:

```text
GET /api/v1/spaces/{space_id}/nodes/{node_id}
GET /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
PUT/PATCH /api/v1/spaces/{space_id}/nodes/{node_id}/metadata
```

규칙:

- metadata는 content가 아니다.
- 민감한 값은 metadata에 넣지 않는다는 안내를 UI에 둘 수 있다.
- Inspector는 content editor가 아니다.

### AgentPanel

현재 문맥에서 agent 작업을 보조하는 view다.

정보 후보:

- current node context
- prompt/input
- task status
- result/diff proposal

초기에는 구체 기능이 확정될 때까지 최소 view로 둔다.

## StatusBar

`StatusBar`는 짧은 상태만 표시한다.

정보:

- save status
- sync/network status
- current space
- active agent status

규칙:

- 긴 메시지나 목록을 표시하지 않는다.
- 모바일에서는 compact 또는 hidden presentation을 사용할 수 있다.
