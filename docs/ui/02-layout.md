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
- agent 실행 로직을 직접 갖지 않는다.

### TitleBar

상단 전역 바다.

담는 것:

- 현재 space 표시
- 전역 command/search 진입점
- account action
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
├─ SpaceRailList   # scrollable
└─ RailFooter
   ├─ AccountButton
   └─ SettingsButton
```

역할:

- 접근 가능한 Space를 표시한다.
- 현재 active space를 표시한다.
- Space가 많으면 상단~중간 영역에서 스크롤한다.
- account/settings는 하단에 고정한다.

`ActivityRail`은 파일 트리가 아니다. 파일 트리는 `PrimarySidebar` 안의 `NodeTreeView`다.

### PrimarySidebar

active space의 탐색 view를 표시하는 좌측 사이드바다.

기본 구성:

```text
PrimarySidebar
├─ SidebarHeader
├─ SidebarTabs
│  ├─ Tree
│  └─ Recent
└─ SidebarContent
```

`Tree`는 folder/text/file 계층을 보여준다. `Recent`는 최근 수정된 node를 단순 list로 보여준다.

### EditorArea

현재 열린 node의 주 작업 영역이다.

기본 구조:

```text
EditorArea
├─ EditorTabBar
├─ DocumentHeader
└─ EditorViewport
```

`EditorArea`는 영역 이름이고, 실제 편집기는 `TextEditor`다.

### AuxiliarySidebar

우측 보조 사이드바다. 현재 선택된 node나 editor context에 대한 보조 정보를 표시한다.

기본 view:

```text
inspector
agent
references
outline
```

초기 기본 view는 다음 두 개다.

```text
InspectorPanel
AgentPanel
```

`AuxiliarySidebar`는 오른쪽 전체 영역이고, `AgentPanel`은 그 안의 view 중 하나다.

### StatusBar

앱 하단의 얇은 상태 표시줄이다.

담는 것:

- save status
- sync status
- current path
- active agent status
- word/line count

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
│ TitleBar                   │
├────────────────────────────┤
│ EditorArea                 │
│                            │
│                            │
├────────────────────────────┤
│ MobileNav / compact status │
└────────────────────────────┘
```

```text
ActivityRail      hidden, presented as space switcher or command menu
PrimarySidebar    hidden, presented as drawer
EditorArea        visible main
AuxiliarySidebar  hidden, presented as sheet
StatusBar         compact or hidden
```

Mobile에서 문서가 열려 있지 않으면 `EditorArea`는 empty state를 표시한다. `PrimarySidebar` drawer에서 node를 선택하면 drawer를 닫고 `EditorArea`로 돌아온다. `AuxiliarySidebar` sheet는 inspector, agent, references 같은 보조 view를 일시적으로 보여준다.

## Presentation mapping

| Layout role | Desktop presentation | Tablet presentation | Mobile presentation |
|---|---|---|---|
| `ActivityRail` | scrollable space rail | compact space rail | space switcher or command menu |
| `PrimarySidebar` | fixed left sidebar | fixed or collapsible sidebar | drawer |
| `EditorArea` | center pane | main pane | full main screen |
| `AuxiliarySidebar` | fixed right sidebar | overlay | sheet |
| `StatusBar` | bottom status bar | compact status bar | compact or hidden |

## Breakpoint policy

정확한 breakpoint 값은 구현에서 조정할 수 있다. 문서 정본은 viewport별 역할 전환만 고정한다.

```text
desktop -> full workbench
tablet  -> auxiliary overlay
mobile  -> editor-first single screen
```
