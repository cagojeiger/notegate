# UI visual system

이 문서는 notegate 대시보드의 기본 visual template이다. Google/Material Design 3는 토큰화, 여백, 단순한 상호작용 색, 명확한 상태 표현을 참고한다. Notegate는 Google UI를 복제하지 않고, dark-first workbench 제품에 맞게 적용한다.

## 1. Visual direction

Notegate의 기본 인상:

```text
focused
calm
technical
private
agent-friendly
```

원칙:

1. 작업 화면은 장식보다 읽기와 편집에 집중한다.
2. 색은 상태와 action을 구분하는 데만 사용한다.
3. 레이아웃 경계는 shadow보다 surface 차이와 hairline border로 표현한다.
4. 컴포넌트 값은 임의 픽셀이 아니라 token에서 가져온다.
5. Visual style은 `01-glossary.md`, `02-layout.md`, `03-information.md`, `04-flows.md`의 구조를 바꾸지 않는다.

## 2. Theme baseline

초기 테마는 dark-first다.

| 역할 | 값 | 용도 |
|---|---:|---|
| `bg` | `#0b0f14` | 앱 최상위 배경 |
| `surface` | `#101722` | `TitleBar`, `StatusBar`, 기본 surface |
| `panel` | `#151d28` | `PrimarySidebar`, `AuxiliarySidebar`, card |
| `panel-strong` | `#1b2432` | active row, selected tab |
| `border` | `#263241` | 영역 경계, hairline |
| `border-strong` | `#334155` | focus/resize/active split 경계 |
| `text` | `#e6edf3` | 본문, 주요 label |
| `muted` | `#8b98a8` | 보조 label, metadata |
| `faint` | `#5f6b7a` | placeholder, disabled text |
| `primary` | `#4f8cff` | 주요 action, focus |
| `primary-hover` | `#6aa0ff` | primary hover |
| `success` | `#22c55e` | 저장 완료, 정상 상태 |
| `warning` | `#f59e0b` | conflict, 주의 |
| `danger` | `#ef4444` | 삭제, destructive action |

규칙:

- `primary`는 action과 focus에만 사용한다.
- 삭제, 영구 손실 가능 action은 `danger` 계열을 사용한다.
- node kind별 색을 과하게 만들지 않는다. kind는 아이콘과 label로 우선 구분한다.
- metadata 값은 `muted`를 기본으로 한다.

## 3. Typography

기본 font stack:

```css
--font-ui: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
--font-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
```

| 역할 | 크기 | 굵기 | 용도 |
|---|---:|---:|---|
| `text-xs` | 12px | 400/500 | metadata, status, small label |
| `text-sm` | 14px | 400/500 | tree row, button, tab |
| `text-base` | 16px | 400 | editor preview body |
| `title-sm` | 16px | 600 | sidebar section title, tab title |
| `title-md` | 20px | 600 | document heading |
| `display-sm` | 28px | 650 | document title in preview |

규칙:

- Workbench UI는 12~14px 중심으로 밀도를 유지한다.
- 문서 preview는 16px 이상으로 읽기성을 우선한다.
- 코드, JSON, YAML, TOML, metadata preview는 mono font를 사용한다.
- 제목을 강조할 때 색을 늘리지 말고 크기와 weight를 사용한다.

## 4. Spacing and shape

기본 spacing은 4px grid다.

```text
1 = 4px
2 = 8px
3 = 12px
4 = 16px
6 = 24px
8 = 32px
```

Radius:

| token | 값 | 용도 |
|---|---:|---|
| `radius-sm` | 6px | tree row, tab |
| `radius-md` | 10px | button, input |
| `radius-lg` | 14px | card, inspector section |
| `radius-full` | 9999px | avatar, pill badge |

규칙:

- 큰 layout region은 둥글게 만들지 않는다. 영역은 직선 경계로 정렬한다.
- 작은 interactive target은 radius를 사용해 클릭 가능성을 드러낸다.
- shadow는 기본적으로 사용하지 않는다. floating menu/dialog에만 제한적으로 사용한다.

## 5. Layout alignment rules

시각 정렬은 픽셀 값보다 관계로 정의한다.

1. `TitleBar`, `EditorGroupHeader`, `StatusBar`는 workbench grid와 수평 경계를 맞춘다.
2. `ActivityRail`, `PrimarySidebar`, `EditorArea`, `AuxiliarySidebar`는 세로 경계를 명확히 가진다.
3. `EditorGroup`이 여러 개일 때 각 group header 높이는 같아야 한다.
4. `PrimarySidebar` 내부의 `TreeSection`과 `RecentSection`은 각자 scroll을 가진다.
5. `TreeSection:RecentSection` 기본 비율은 `2:1`이다.
6. `SectionResizeHandle`과 sidebar resize handle은 hover/focus 상태를 명확히 보여준다.
7. Node path, byte, line 같은 문서 metadata는 기본적으로 `InspectorPanel` 책임이다.

## 6. Component template

초기 구현에서 필요한 기본 컴포넌트:

```text
Button
IconButton
Input
Dialog
ContextMenu
Tabs
TreeRow
SidebarSection
ResizeHandle
EditorGroupHeader
EditorViewport
InspectorCard
StatusIndicator
```

### Button

Variant:

```text
primary
secondary
ghost
danger
```

규칙:

- `primary`는 한 화면의 핵심 action에만 사용한다.
- tree row 안 action은 `ghost` 또는 icon-only로 둔다.
- destructive confirm dialog의 최종 action만 `danger`를 사용한다.

### TreeRow

표시 정보:

```text
expand/collapse control
kind icon
name
optional secondary value
selected/focused state
```

규칙:

- name은 왼쪽 정렬한다.
- click은 선택/open, folder expand/collapse는 별도 affordance를 제공한다.
- keyboard navigation을 고려해 focus ring을 가진다.

### EditorGroupHeader

구성:

```text
left: node identity
center: empty/reserved
right: group actions
```

규칙:

- node path 전체를 반복 노출하지 않는다.
- Inspector와 중복되는 metadata를 header에 넣지 않는다.
- split이 최대 3개에 도달하면 add action은 disabled 상태로 보인다.

### InspectorCard

용도:

```text
node property
metadata JSON
policy/security note
future agent context
```

규칙:

- metadata는 content가 아니다.
- encrypted text의 plaintext를 inspector에 표시하지 않는다.

## 7. Interaction states

| 상태 | 표현 |
|---|---|
| Loading | skeleton row 또는 spinner. layout jump를 만들지 않는다 |
| Empty | 짧은 설명 + 1개 primary action |
| Error | 원인 + 다음 행동. backend message를 그대로 길게 노출하지 않는다 |
| Success | `StatusBar` 또는 toast에 짧게 표시 |
| Disabled | opacity만 낮추지 말고 tooltip/aria-label로 이유를 제공한다 |
| Conflict | warning color + 다시 읽기/비교 action |
| Dirty draft | editor header 또는 status에 저장 전 상태를 표시한다 |

## 8. Responsive behavior

Viewport별 presentation:

| 범위 | 동작 |
|---|---|
| Desktop | full workbench: rail/sidebar/editor/auxiliary |
| Tablet | `AuxiliarySidebar`는 overlay 또는 숨김으로 전환 가능 |
| Mobile | `EditorArea` 중심 단일 화면. sidebar/auxiliary는 sheet 또는 route-level view |

규칙:

- Layout role은 유지한다. Mobile에서도 `PrimarySidebar` 개념은 사라지지 않고 presentation만 sheet/list로 바뀐다.
- Touch target은 최소 44px 이상을 목표로 한다.
- Hover에만 의존하는 action은 mobile 대체 진입점을 가져야 한다.

## 9. Tailwind mapping

Tailwind는 CSS variable을 참조한다. 색 값을 컴포넌트에 직접 흩뿌리지 않는다.

```ts
// tailwind.config.ts concept
colors: {
  bg: "var(--ng-bg)",
  surface: "var(--ng-surface)",
  panel: "var(--ng-panel)",
  border: "var(--ng-border)",
  text: "var(--ng-text)",
  muted: "var(--ng-muted)",
  primary: "var(--ng-primary)",
  danger: "var(--ng-danger)",
}
```

초기 CSS variable template:

```css
:root {
  --ng-bg: #0b0f14;
  --ng-surface: #101722;
  --ng-panel: #151d28;
  --ng-panel-strong: #1b2432;
  --ng-border: #263241;
  --ng-border-strong: #334155;
  --ng-text: #e6edf3;
  --ng-muted: #8b98a8;
  --ng-faint: #5f6b7a;
  --ng-primary: #4f8cff;
  --ng-primary-hover: #6aa0ff;
  --ng-success: #22c55e;
  --ng-warning: #f59e0b;
  --ng-danger: #ef4444;
}
```

## 10. Do / Don't

Do:

- workbench boundary를 명확히 맞춘다.
- 정보 밀도는 높게, 본문 읽기 영역은 넓게 둔다.
- action 색은 적게 사용한다.
- 모든 수치는 token으로 승격 가능한 값으로 둔다.
- error와 empty state는 다음 행동을 알려준다.

Don't:

- Google 색/로고/폰트를 그대로 복제하지 않는다.
- `Panel`, `Content`, `Body` 같은 일반 이름을 새로 만들지 않는다.
- Inspector에 있어야 할 metadata를 header/status에 중복 표시하지 않는다.
- hover-only action만 제공하지 않는다.
- shadow와 gradient로 영역을 구분하지 않는다.

## 11. Open questions

- Light theme을 v1에 포함할지 여부.
- Command palette를 초기 구현에 포함할지 여부.
- Agent runtime 기능이 확정될 때 오른쪽 예약 영역과 `AuxiliarySidebar` 중 어디까지 노출할지.
- 파일 preview가 필요한 media type 목록.
