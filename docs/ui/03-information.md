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

- current space name
- current node path 또는 page title
- global command/search entry
- account menu
- sync/auth 상태가 필요할 때 짧은 indicator

규칙:

- 긴 목록이나 검색 결과를 표시하지 않는다.
- node tree를 표시하지 않는다.
- 모바일에서는 space/path 표시를 축약할 수 있다.

## ActivityRail

`ActivityRail`은 Space 전환과 전역 account/settings 진입을 담당한다.

구성:

```text
ActivityRail
├─ SpaceRailList   # 상단~중간, scrollable
└─ RailFooter      # 하단 고정
   ├─ AccountButton
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
- Account/settings는 스크롤 영역에 들어가지 않고 하단에 고정한다.
- Space create/delete/rename은 rail 안에서 직접 처리하지 않고 dashboard의 명시적 관리 화면에서 처리한다.

### RailFooter

하단 고정 항목:

```text
account
settings
```

규칙:

- `account`는 현재 user/account menu를 연다.
- `settings`는 settings 화면 또는 설정 route로 이동한다.
- `ActivityRail`은 파일 트리를 표시하지 않는다.

## PrimarySidebar

`PrimarySidebar`는 active space의 탐색 view를 표시한다.

기본 구성:

```text
PrimarySidebar
├─ SpaceSwitcher
├─ SidebarTabs
│  ├─ Tree
│  └─ Recent
└─ SidebarContent
```

#### Tree tab

`Tree`는 Space 안 folder/text/file 계층을 보여준다.

정보:

- folder/text/file name
- kind icon
- selected state
- has_children indicator
- optional byte_len 또는 line_count
- pagination/load more state

Backend:

```text
GET /api/v1/spaces/{space_id}/nodes/{folder_id}/children?limit=...&cursor=...
```

규칙:

- 한 번에 전체 tree를 펼쳐서 렌더링하지 않는다.
- 화면에 보이는 folder만 children을 요청한다.
- folder children은 pagination cursor로 추가 로드한다.
- backend children page 기본값은 100, 최대 200이다.
- 폴더 하나의 direct children이 많아도 UI 높이는 무한히 커지지 않는다.
- `Load more` 또는 가상 스크롤로 visible item 수를 제한한다.

#### Recent tab

`Recent`는 최근 수정된 node를 단순 list로 보여준다.

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

- open tabs
- document header
- breadcrumb/path
- node name
- save state
- editor viewport 또는 preview viewport

### Folder selected

Folder를 선택하면 folder summary 또는 empty state를 보여준다.

정보:

- folder name
- path
- child count summary가 있으면 표시
- create action

### Text selected

Text를 선택하면 text editor 또는 preview를 보여준다.

정보:

- content
- storage_format: plain 또는 encrypted
- content_sha256
- line_count
- byte_len
- updated_at

규칙:

- plain Text는 editor에서 읽고 수정한다.
- encrypted Text는 서버가 복호화하지 않으므로 encrypted 상태를 명확히 표시한다.
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
- current path
- sync/network status
- active agent status
- text line/word count

규칙:

- 긴 메시지나 목록을 표시하지 않는다.
- 모바일에서는 compact 또는 hidden presentation을 사용할 수 있다.
