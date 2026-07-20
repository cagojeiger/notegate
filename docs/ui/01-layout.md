# UI 레이아웃

## AppRoot

```text
AppRoot
├─ AuthScreen
└─ AppShell
```

- `/api/v1/me` 성공 시 AppShell.
- 401 또는 로그아웃 시 AuthScreen.
- AuthScreen은 AppShell 내부에 넣지 않는다.

## Desktop

```text
┌──────────────────────────────────────────────────────┐
│ TitleBar                                             │
├──────┬───────────────┬────────────────┬──────────────┤
│      │               │                │              │
│ Acti │ Primary       │ EditorArea     │ Auxiliary    │
│ vity │ Sidebar       │ 1..3 groups    │ Sidebar      │
│ Rail │               │                │              │
├──────┴───────────────┴────────────────┴──────────────┤
│ StatusBar                                            │
└──────────────────────────────────────────────────────┘
```

## TitleBar

포함:

- 제품명과 현재 space 이름.
- PrimarySidebar 토글.
- EditorGroup 분할 버튼.
- AuxiliarySidebar 토글.
- theme 토글.

규칙:

- 중앙 검색 영역은 아직 비워둔다.
- 현재 node path는 표시하지 않는다.
- Inspector 버튼을 EditorGroup 안에 중복 배치하지 않는다.

## ActivityRail

```text
ActivityRail
├─ SpaceRailList
├─ SpaceAddButton
└─ SettingsButton
```

규칙:

- SpaceRailList는 스크롤 가능하다.
- SpaceAddButton은 space 목록 바로 아래에 둔다.
- 진행 중이거나 실패한 file transfer는 UploadProgressDock에서 표시한다.
- SettingsButton은 하단에 고정한다.
- space reorder는 desktop drag-and-drop으로 한다.

## PrimarySidebar

```text
PrimarySidebar
├─ SidebarHeader
└─ SidebarContent
   ├─ FilesSection
   ├─ SidebarSectionResizeHandle
   └─ RecentSection
```

규칙:

- sidebar width는 사용자가 조절할 수 있다.
- Files와 Recent는 독립적으로 스크롤한다.
- 기본 높이 비율은 Files:Recent = 2:1.
- 중간 divider가 비율 조절 handle이다.
- root `/`는 행으로 보이지 않는다.
- Files는 collapse-all을 제공한다.
- Recent는 목록/압축 보기 전환을 제공한다.

## EditorArea

```text
EditorArea
└─ EditorGroup[1..3]
   ├─ EditorGroupHeader
   └─ EditorViewport
```

규칙:

- non-mobile은 최대 3개 group을 split으로 표시한다.
- mobile은 focused presentation을 사용하고 한 번에 1개 group만 표시한다.
- 새 group은 활성 group 오른쪽에 추가된다.
- 3개일 때 분할 버튼은 disabled 상태다.
- 빈 group도 active 상태가 보여야 한다.
- text는 preview mode로 열린다.
- edit mode는 preview를 같은 group 안에서 대체한다.
- group close는 header에서 처리한다.

## AuxiliarySidebar

포함:

- `InspectorPanel`

규칙:

- node가 없어도 빈 Inspector를 보여준다.
- desktop/tablet은 inline docked panel이다.
- mobile은 overlay/sheet다.
- agent 관리는 Settings에서 한다.

## StatusBar

포함:

- 앱 준비/저장 상태.
- 현재 space 이름.

포함하지 않음:

- node path.
- byte count.
- line count.
- updated timestamp.

node 상세 정보는 Inspector가 담당한다.

## UploadProgressDock

진행 중인 file upload는 앱 범위의 임시 panel에서 파일별로 보여준다.

- desktop/tablet은 오른쪽 아래에 표시한다.
- mobile은 하단 space bar 위에 표시한다.
- 대상 space와 folder path, 진행률, 상태를 보여준다.
- 진행 중 항목은 취소할 수 있고 실패 항목은 재시도하거나 닫을 수 있다.
- 완료 항목은 잠시 표시한 뒤 자동으로 제거한다.
- History는 완료된 Changes와 Audit만 담당한다.

## 반응형

| 화면 | 규칙 |
|---|---|
| Desktop | docked sidebars, split editor, 최대 3 editor groups |
| Tablet | desktop과 같은 non-mobile workbench path, docked sidebars, split editor |
| Mobile | editor 우선, sidebars는 overlay/sheet, group은 하나씩 표시 |
