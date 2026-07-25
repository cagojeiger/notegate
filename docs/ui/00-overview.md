# UI 개요

NoteGate UI는 개인 문서 저장소를 탐색하고 편집하는 데스크톱 우선 대시보드다.

## 목표

- 파일시스템처럼 이해되는 문서 작업 공간.
- 문서 읽기와 편집이 중심인 조용한 화면.
- API, MCP, 브라우저가 같은 데이터 모델을 공유.
- 모바일은 전체 기능보다 읽기/간단 조작을 우선.

## 화면 구조

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

## 레이아웃 용어

| 이름 | 의미 |
|---|---|
| `AppRoot` | 인증 상태에 따라 AuthScreen 또는 AppShell을 보여주는 루트 |
| `AuthScreen` | 로그인 화면 |
| `AppShell` | 로그인 이후 전체 앱 껍데기 |
| `TitleBar` | 상단 앱 바 |
| `Workbench` | 실제 작업 영역 |
| `ActivityRail` | 좌측 space rail |
| `PrimarySidebar` | Files/Recent 사이드바 |
| `EditorArea` | 문서/파일을 여는 중앙 작업 영역 |
| `AuxiliarySidebar` | Inspector 같은 보조 정보 영역 |
| `StatusBar` | 하단 상태 표시줄 |

## 세부 용어

| 이름 | 의미 |
|---|---|
| `SpaceRailList` | ActivityRail의 space 목록 |
| `SpaceAddButton` | 새 space 버튼 |
| `FilesSection` | 트리 기반 node 탐색 영역. 화면 라벨은 `Files` |
| `RecentSection` | 최근 변경 node 목록. 화면 라벨은 `Recent` |
| `EditorGroup` | EditorArea 안의 독립 pane |
| `EditorGroupHeader` | EditorGroup 상단 헤더 |
| `EditorViewport` | 실제 본문이 렌더링되는 영역 |
| `InspectorPanel` | 선택 node의 속성과 metadata 표시 |
| `SettingsModal` | Account, Agents, MCP 설정 화면 |
| `StructuredPreview` | JSON/JSONL/YAML/TOML tree/source 뷰 |

## 기준 규칙

- 로그인 화면은 AppShell과 분리한다.
- 위치명보다 역할명을 쓴다.
- `EditorArea`는 본문 작업 공간이다.
- `PrimarySidebar`는 Files와 Recent를 담당한다.
- `AuxiliarySidebar`는 Inspector와 보조 정보를 담당한다.
- node path, byte, line 같은 상세 정보는 Inspector에 둔다.

## 금지/비추천 이름

- `LeftPanel`
- `RightPanel`
- `MainPanel`
- `SideMenu`
- `Content`
- `Body`
- `BottomPanel`
- `EditorInfoBar`
