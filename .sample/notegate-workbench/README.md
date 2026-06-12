# Notegate Workbench sample

정적 UI 샘플이다. 실제 frontend 앱이 아니라 `docs/ui`의 레이아웃 결정을 클릭해보기 위한 프로토타입이다.

## Run

```bash
cd .sample/notegate-workbench
python3 -m http.server 4173
```

Open:

```text
http://localhost:4173
```

## What to check

- `ActivityRail`: Space 목록 동적 스크롤, Space 추가 버튼, 하단 settings 고정
- `PrimarySidebar`: Resizable PrimarySidebar width, Tree/Recent independent scroll sections, collapse-all and recent density controls, draggable Tree/Recent divider, folder children scroll pagination, ArrowUp/ArrowDown selection, Tree context menu
- `EditorArea`: text/file/folder 표시, Preview/Edit mode toggle, EditorGroupHeader-based independent editor groups, up to 3 panes, close by header ×
- Top-right layout controls: PrimarySidebar toggle, add EditorGroup to the right, AuxiliarySidebar toggle
- `AuxiliarySidebar`: Inspector/Agent 탭, hide/show
- Mobile width: tree drawer, auxiliary bottom sheet
