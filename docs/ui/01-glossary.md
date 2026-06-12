# UI glossary

이 문서는 notegate UI 용어 정본이다. 같은 UI 영역은 항상 같은 이름으로 부른다.

## Layout terms

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

| 표준명 | 한국어 설명 | 의미 |
|---|---|---|
| `AppRoot` | 앱 루트 | 인증 상태에 따라 `AuthScreen` 또는 `AppShell`을 렌더링하는 최상위 entry |
| `AuthScreen` | 인증 화면 | 로그인, 로그인 성공, 인증 오류를 보여주는 독립 화면. `Workbench`를 포함하지 않는다 |
| `AppShell` | 앱 셸 | 로그인 이후 대시보드 전체 화면의 최상위 레이아웃 컨테이너 |
| `TitleBar` | 상단 바 | 앱 전역 action 영역. space 이름, command/search, account action을 둔다 |
| `Workbench` | 작업대 | 사용자가 문서를 탐색하고 편집하는 주 작업 영역 |
| `ActivityRail` | 액티비티 레일 | 좌측의 좁은 전역 rail. Space 목록을 스크롤로 표시하고 하단에 account/settings를 둔다 |
| `PrimarySidebar` | 주 사이드바 | active space의 node tree와 recent list를 표시하는 좌측 사이드바 |
| `EditorArea` | 에디터 영역 | 현재 열린 node의 주 작업 영역. tab, header, editor viewport를 포함한다 |
| `AuxiliarySidebar` | 보조 사이드바 | 우측 보조 영역. Inspector, Agent, References 같은 contextual view를 표시한다 |
| `StatusBar` | 상태 바 | 앱 하단 상태 표시줄. 저장 상태, 동기화 상태, 현재 경로, agent 상태를 표시한다 |

## View terms

| 표준명 | 의미 |
|---|---|
| `NodeTreeView` | Space 안 folder/text/file tree를 보여주는 view |
| `InspectorPanel` | 선택된 node의 metadata와 속성을 보여주는 보조 panel |
| `AgentPanel` | AI agent 작업 상태와 대화를 보여주는 보조 panel |
| `ReferencesPanel` | 연결 문서, backlink, reference를 보여주는 보조 panel |
| `OutlineView` | 현재 text의 heading outline을 보여주는 view |
| `TextEditor` | plain text/markdown 편집기 |
| `MarkdownPreview` | markdown preview view |
| `FilePreview` | file metadata 또는 preview view |

## Naming rules

- layout 이름은 영역의 위치와 책임을 나타낸다.
- view 이름은 실제로 보여주는 내용을 나타낸다.
- `PrimarySidebar` 안에 `NodeTreeView`가 들어간다.
- `AuxiliarySidebar` 안에 `InspectorPanel`, `AgentPanel`, `ReferencesPanel`이 들어간다.
- `EditorArea` 안에 `TextEditor`, `MarkdownPreview`, `FilePreview`가 들어간다.
- 오른쪽 전체 영역은 `AuxiliarySidebar`라고 부른다. `AgentPanel`은 그 안의 view 중 하나다.

## Avoided names

다음 이름은 새 component/state 이름에 사용하지 않는다.

```text
LeftPanel
RightPanel
MenuPanel
SideMenu
FilePanel
MainPanel
Content
Body
Panel  # 단독 사용 금지. InspectorPanel처럼 구체화해서 사용한다.
```
