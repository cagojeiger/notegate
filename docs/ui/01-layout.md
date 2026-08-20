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

- 중앙 검색 영역은 비어 있다.
- 현재 node path는 표시하지 않는다.
- Inspector 버튼을 EditorGroup 안에 중복 배치하지 않는다.

## ActivityRail

```text
ActivityRail
├─ SpaceRailList
├─ SpaceAddButton
├─ HistoryButton
└─ SettingsButton
```

규칙:

- SpaceRailList는 스크롤 가능하다.
- SpaceAddButton은 space 목록 바로 아래에 둔다.
- 진행 중이거나 실패한 file transfer는 UploadProgressDock에서 표시한다.
- HistoryButton과 SettingsButton은 하단에 고정한다.
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

### Links

- 내부 링크는 `Links from this document`와 `Links to this document` 두 영역으로 나눈다.
- Text 문서에서는 두 영역을 기본으로 모두 열고 각각 독립적으로 스크롤한다.
- 중간 divider로 두 영역의 높이 비율을 조절하며, 키보드 조절도 지원한다.
- 한 영역을 접으면 다른 영역이 남은 높이를 모두 사용하고, 다시 열면 이전 비율을 복원한다.
- 분할 비율은 세션 공통 레이아웃 값으로 유지하며, 문서를 바꾸면 두 영역은 다시 기본으로 열린다.
- 각 영역은 독립 cursor pagination을 사용하며 스크롤 끝에 가까워지면 다음 페이지를 요청한다.
- Folder와 File은 incoming 링크 영역만 표시하고 전체 높이를 사용한다.
- 외부 웹 링크는 이 화면의 현재 범위에 포함하지 않는다.

## StatusBar

포함:

- 앱 준비/저장 상태.
- 현재 space 이름.
- 현재 space의 전체 item 수와 Text/File 합산 사용량.
- 새 item의 검색 포함 기본값과 새 Text의 암호화 기본값.

포함하지 않음:

- node path.
- 현재 node의 byte count.
- line count.
- updated timestamp.

Text/File 한도는 서로 독립이므로 StatusBar에서 합산 비율을 만들지 않는다. 상세 사용량과 node 정보는 Inspector가 담당한다.

## UploadProgressDock

진행 중인 file upload는 앱 범위의 임시 panel에서 파일별로 보여준다.

- desktop/tablet은 오른쪽 아래에 표시한다.
- mobile은 하단 space bar 위에 표시한다.
- 대상 space와 folder path, 진행률, 상태를 보여준다.
- 진행 중 항목은 취소할 수 있고 실패 항목은 재시도하거나 닫을 수 있다.
- 완료 항목은 잠시 표시한 뒤 자동으로 제거한다.
- History는 Changes, Audit, MCP, queue Jobs 이력을 담당한다. Jobs는 활성 작업이 있을 때만 자동 갱신한다.

## RecordingDock

활성 audio recording은 download/upload progress 창과 같은 placement grammar의 독립 panel로 표시한다.

- header에는 `Recording` 또는 `Paused`, recorded duration, 실제 microphone level, collapse/expand를 보여준다. recorded duration은 pause 동안 증가하지 않는다.
- expanded body에는 root target filename/path, segment 수와 누적 pause 시간, `Pause`/`Resume`, `Discard`, `Stop & save`를 보여준다.
- desktop/tablet은 오른쪽 아래에서 `UploadProgressDock`과 같은 24 rem 폭을 사용하고, 두 panel이 함께 있으면 Recording을 위에 쌓는다.
- collapse 상태에서도 `Recording`/`Paused`와 recorded duration은 계속 보여준다. document를 읽을 공간이 필요할 때 사용자가 panel을 접을 수 있다.
- mobile에서는 floating overlay를 강제하지 않고 bottom stack의 full-width panel로 표시한다.
- `Stop & save` 뒤에는 panel을 제거하고 생성된 File을 `UploadProgressDock`에서 표시한다.

## 반응형

| 화면 | 규칙 |
|---|---|
| Desktop | docked sidebars, split editor, 최대 3 editor groups |
| Tablet | desktop과 같은 non-mobile workbench path, docked sidebars, split editor |
| Mobile | editor 우선, sidebars는 overlay/sheet, group은 하나씩 표시 |
