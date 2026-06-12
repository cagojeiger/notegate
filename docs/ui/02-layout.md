# UI layout

notegate는 인증 화면과 로그인 이후 대시보드를 분리한다. 대시보드는 workbench형 레이아웃을 사용한다. 레이아웃은 화면의 큰 영역과 책임만 정의한다.

## App root

```text
AppRoot
├─ AuthScreen
└─ AppShell
   ├─ TitleBar
   ├─ Workbench
   │  ├─ ActivityRail
   │  ├─ PrimarySidebar
   │  ├─ EditorArea
   │  └─ AuxiliarySidebar
   └─ StatusBar
```

`AuthScreen`은 로그인 전용 화면이다. `AppShell`은 로그인 이후 대시보드 화면이다. Login은 `AppShell` 내부 route가 아니다.

## Dashboard layout tree

```text
AppShell
├─ TitleBar
├─ Workbench
│  ├─ ActivityRail
│  ├─ PrimarySidebar
│  ├─ EditorArea
│  └─ AuxiliarySidebar
└─ StatusBar
```

## Screen map

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ TitleBar                                                                     │
├──────┬──────────────────┬──────────────────────────────────┬────────────────┤
│      │                  │                                  │                │
│ Acti │ PrimarySidebar   │ EditorArea                       │ AuxiliarySidebar│
│ vity │                  │                                  │                │
│ Rail │                  │                                  │                │
│      │                  │                                  │                │
├──────┴──────────────────┴──────────────────────────────────┴────────────────┤
│ StatusBar                                                                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Region responsibilities

### AppRoot

인증 상태에 따라 `AuthScreen` 또는 `AppShell`을 렌더링한다.

```text
not logged in / auth callback / auth error -> AuthScreen
logged in                                 -> AppShell
```

### AuthScreen

로그인 전용 독립 화면이다.

담는 것:

- login start
- login success
- auth error
- not registered 안내

담지 않는 것:

- `TitleBar`
- `Workbench`
- `ActivityRail`
- `PrimarySidebar`
- `EditorArea`
- `AuxiliarySidebar`
- `StatusBar`

### AppShell

로그인 이후 대시보드 화면의 최상위 레이아웃이다.

책임:

- `TitleBar`, `Workbench`, `StatusBar`를 배치한다.
- 로그인 이후 dashboard 화면의 frame을 만든다.

하지 않는 일:

- node tree를 직접 렌더링하지 않는다.
- text/file content를 직접 수정하지 않는다.

### TitleBar

상단 전역 바다.

담는 것:

- 현재 space 또는 product context 표시
- 레이아웃 제어: `PrimarySidebar` 표시, `EditorGroup` 추가, `AuxiliarySidebar` 표시
- 전역 알림 또는 상태 action

담지 않는 것:

- node tree
- editor content
- 긴 검색 결과

### Workbench

사용자가 실제로 탐색하고 편집하는 주 작업 영역이다.

구성:

```text
Workbench
├─ ActivityRail
├─ PrimarySidebar
├─ EditorArea
└─ AuxiliarySidebar
```

### ActivityRail

좌측의 좁은 전역 rail이다. Space 전환을 가장 빠르게 수행하는 영역이다.

구성:

```text
ActivityRail
├─ SpaceRailList   # scrollable, dynamic height
├─ SpaceAddButton  # directly after SpaceRailList
└─ RailFooter
   └─ SettingsButton
```

역할:

- 접근 가능한 Space를 표시한다.
- 현재 active space를 표시한다.
- Space가 많으면 `SpaceRailList` 내부만 스크롤한다.
- Space 추가 버튼은 Space list 실제 항목 바로 아래에 항상 표시한다.
- Settings는 하단에 고정한다. Account 관련 진입도 Settings에서 처리한다.

`ActivityRail`은 파일 트리가 아니다. 파일 트리는 `PrimarySidebar` 안의 `NodeTreeView`다.

### PrimarySidebar

active space의 탐색 view를 표시하는 좌측 사이드바다.

기본 구성:

```text
PrimarySidebar
├─ SidebarHeader
└─ SidebarContent
   ├─ TreeSection
   ├─ SectionResizeHandle
   └─ RecentSection
```

`TreeSection`은 folder/text/file 계층을 보여준다. `RecentSection`은 최근 수정된 node를 단순 list로 보여준다. 두 section은 각각 독립적으로 스크롤하고, 초기 기본 높이 비율은 `TreeSection:RecentSection = 2:1`이다. 사용자가 사이 resize bar를 조절하면 이후 비율은 사용자 설정을 따른다. `PrimarySidebar` 자체의 폭도 사용자가 조절할 수 있다.

### EditorArea

현재 열린 node의 주 작업 영역이다.

기본 구조:

```text
EditorArea
└─ EditorGroup[]
   ├─ EditorGroupHeader
   ├─ EditorViewport
```

`EditorArea`는 영역 이름이다. `EditorGroup`은 독립 pane이며, VSCode처럼 1개에서 최대 3개까지 분할될 수 있다. 각 `EditorGroup`은 열린 node와 preview/edit mode를 따로 가진다. Text는 기본 preview mode로 열리고, 사용자가 edit mode로 전환하면 해당 group 안에서 `TextEditor`를 보여준다. 새 group은 오른쪽에 추가되며, 최대 개수에 도달하면 추가 action은 disabled 상태가 된다. group 제거는 `EditorGroupHeader`의 close action으로 수행한다.

### AuxiliarySidebar

우측 보조 사이드바다. 현재 선택된 node나 editor context에 대한 보조 정보를 표시한다.

기본 view:

```text
inspector
references
outline
```

초기 기본 view는 `InspectorPanel`이다.

`AuxiliarySidebar`는 오른쪽 전체 영역이고, `InspectorPanel`은 그 안의 view 중 하나다.

### StatusBar

앱 하단의 얇은 상태 표시줄이다.

담는 것:

- save status
- current space
- 오른쪽 runtime status 예약 영역


## Tree context actions

`NodeTreeView`는 desktop에서 context menu를 제공할 수 있다. 같은 기능은 keyboard, touch, mobile 사용자를 위해 `PrimarySidebar` header action에서도 접근 가능해야 한다.

생성 대상:

```text
folder context -> 해당 folder 아래
root/empty tree context -> active space root 아래
text/file context -> child 생성 없음
```

기본 생성 action:

```text
New Folder
New Text
Upload File
```

규칙:

- Folder와 root/empty context에서만 child 생성 action을 제공한다.
- Text/File context에서는 open, rename, delete, copy path 같은 node action만 제공한다.
- 생성 후 전체 tree를 reload하지 않고 parent folder children만 갱신한다.
- 생성된 Text/File은 active `EditorGroup`에 열 수 있다.

## Layout controls

전역 레이아웃 제어는 `TitleBar`의 우측에 둔다. 중앙 command/search 영역은 기능이 확정될 때까지 비워둔다. 각 editor group 안에는 중복되는 inspector/layout 버튼을 두지 않는다.

제어 대상:

```text
PrimarySidebar visibility
EditorGroup add / current group count
AuxiliarySidebar visibility
```

규칙:

- 레이아웃 제어 cluster는 desktop, tablet, mobile에서 항상 `TitleBar` 우측 같은 위치에 둔다. Viewport에 따라 좌측 hamburger 등 다른 위치로 옮기지 않는다.
- 같은 control은 viewport별로 위치와 아이콘을 유지하고 동작만 바꾼다. `PrimarySidebar`/`AuxiliarySidebar` 토글은 desktop에서 inline 영역을 열고 닫고, tablet/mobile에서는 같은 control이 overlay/drawer/sheet를 연다.
- `EditorGroup` 추가 action은 active group의 오른쪽에 새 group을 만든다.
- 최대 group 수에 도달하면 추가 action은 disabled 상태를 표시한다.
- 단일 컬럼인 mobile에서는 `EditorGroup` 추가 action을 숨긴다. `PrimarySidebar`/`AuxiliarySidebar` 토글은 mobile에서도 유지한다.
- `AuxiliarySidebar`는 전역 제어로 열고 닫는다. Desktop에서는 editor group 내부에 별도 inspector 버튼을 두지 않는다.
- Account/Profile 진입은 `ActivityRail`의 Settings를 통해 처리하고, `TitleBar`에 별도 account button을 두지 않는다.

## Default visibility

초기 기본값:

```text
TitleBar         visible
ActivityRail     visible
PrimarySidebar   visible
EditorArea       visible
AuxiliarySidebar visible
StatusBar        visible
```

문서가 열려 있지 않아도 `EditorArea`는 유지한다. 이때 `EditorArea`는 empty state를 표시한다.

## Alignment rules

기본 레이아웃은 좌측, 중앙, 우측 영역의 상단 구조가 시각적으로 같은 기준선에 맞아야 한다. 정확한 pixel 값은 구현에서 정하고, 문서 정본은 정렬 관계만 정의한다.

정렬 기준:

```text
PrimarySidebar header
EditorGroup editor group header
AuxiliarySidebar tab header
```

위 세 영역은 같은 높이와 같은 상단 기준선을 가져야 한다.

```text
┌────────────────┬─────────────────────────┬────────────────────┐
│ Sidebar header │ EditorGroup header      │ Auxiliary tabs     │
├────────────────┼─────────────────────────┼────────────────────┤
│ Tree/Recent    │ EditorViewport          │ Auxiliary content  │
```

규칙:

- EditorGroup이 2개 이상이어도 각 group의 header 높이와 기준선은 서로 같아야 한다.
- `PrimarySidebar`의 section content와 `EditorViewport`, `AuxiliarySidebar` content는 같은 기준선 아래에서 시작한다.
- 구체적인 높이, spacing, typography token은 visual guideline에서 정한다.

## Responsive policy

Layout role은 viewport에 따라 바꾸지 않는다. 같은 영역은 desktop, tablet, mobile에서 같은 이름을 유지한다.

```text
Layout role  = 제품 UI에서 맡는 책임
Presentation = 특정 viewport에서 보이는 방식
```

예:

```text
PrimarySidebar   = layout role
mobile drawer    = mobile presentation

AuxiliarySidebar = layout role
mobile sheet     = mobile presentation
```

## Viewport presentations

### Desktop

Desktop은 전체 workbench를 한 화면에 보여준다.

```text
┌──────────────────────────────────────────────────────────────┐
│ TitleBar                                                     │
├──────┬────────────────┬──────────────────────┬──────────────┤
│ Acti │ PrimarySidebar │ EditorArea           │ Auxiliary    │
│ vity │                │                      │ Sidebar      │
│ Rail │                │                      │              │
├──────┴────────────────┴──────────────────────┴──────────────┤
│ StatusBar                                                    │
└──────────────────────────────────────────────────────────────┘
```

```text
ActivityRail      visible fixed
PrimarySidebar    visible fixed
EditorArea        visible main
AuxiliarySidebar  visible, collapsible
StatusBar         visible
```

### Tablet

Tablet은 `EditorArea`와 탐색 흐름을 우선한다. `AuxiliarySidebar`는 고정 영역에서 빠지고 필요할 때 overlay로 열린다.

```text
┌──────────────────────────────────────────────┐
│ TitleBar                                     │
├──────┬────────────────┬──────────────────────┤
│ Acti │ PrimarySidebar │ EditorArea           │
│ vity │                │                      │
│ Rail │                │                      │
├──────┴────────────────┴──────────────────────┤
│ StatusBar                                    │
└──────────────────────────────────────────────┘

AuxiliarySidebar = overlay
```

```text
ActivityRail      visible compact
PrimarySidebar    visible or collapsible
EditorArea        visible main
AuxiliarySidebar  overlay
StatusBar         visible or compact
```

### Mobile

Mobile은 `EditorArea` 중심의 단일 작업 화면이다. 탐색과 보조 정보는 필요할 때만 열린다.

```text
┌────────────────────────────┐
│ TitleBar (layout controls) │
├────────────────────────────┤
│ EditorArea                 │
│                            │
├────────────────────────────┤
│ Space switcher bar         │
└────────────────────────────┘
```

```text
ActivityRail      hidden as rail, presented as bottom space switcher bar
PrimarySidebar    hidden, opened as drawer from TitleBar control
EditorArea        visible main
AuxiliarySidebar  hidden, opened as sheet from TitleBar control
StatusBar         hidden
```

Mobile에서 `ActivityRail`은 좌측 rail 대신 화면 하단의 가로 space switcher bar로 표현한다. 이 bar는 접근 가능한 space 전환(현재 space는 active 표시), space 추가, Settings 진입을 담는다. Mobile에서는 `StatusBar`를 숨기므로 현재 space는 bar의 active 표시로 구분하고, save status는 일시 toast로 표시한다. 레이아웃 제어(`PrimarySidebar`/`AuxiliarySidebar` 토글)와는 분리하며, 레이아웃 제어는 `TitleBar` 우측에 둔다.

bar 안의 배치는 세로 `ActivityRail`과 같은 순서를 유지한다.

```text
[ space 목록 (scroll) ] │ [ ＋ space 추가 ] ……… [ ⚙ Settings ]
```

- `SpaceRailList`는 좌측에서 스크롤한다.
- `SpaceAddButton`(`＋`)은 space 목록 끝에 바로 붙고, space가 늘면 목록과 함께 오른쪽으로 이동한다.
- Settings(`⚙`)는 bar의 가장 오른쪽에 고정한다.
- space 목록과 `＋`, `Settings`는 각각 구분선으로 나눈다.

Mobile에서 문서가 열려 있지 않으면 `EditorArea`는 empty state를 표시한다. `PrimarySidebar` drawer와 `AuxiliarySidebar` sheet는 `TitleBar` 우측의 같은 레이아웃 제어로 연다. Mobile 전용 별도 nav 버튼은 두지 않는다. `PrimarySidebar` drawer에서 node를 선택하면 drawer를 닫고 `EditorArea`로 돌아온다. `AuxiliarySidebar` sheet는 inspector, references 같은 보조 view를 일시적으로 보여준다.

## Presentation mapping

| Layout role | Desktop presentation | Tablet presentation | Mobile presentation |
|---|---|---|---|
| `ActivityRail` | scrollable space rail | compact space rail | bottom space switcher bar |
| `PrimarySidebar` | fixed left sidebar | fixed or collapsible sidebar | drawer |
| `EditorArea` | center pane | main pane | full main screen |
| `AuxiliarySidebar` | fixed right sidebar | overlay | sheet |
| `StatusBar` | bottom status bar | compact status bar | hidden |

## Breakpoint policy

정확한 breakpoint 값은 구현에서 조정할 수 있다. 문서 정본은 viewport별 역할 전환만 고정한다.

```text
desktop -> full workbench
tablet  -> auxiliary overlay
mobile  -> editor-first single screen
```
