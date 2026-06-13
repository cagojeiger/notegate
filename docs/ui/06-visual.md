# UI visual style

notegate 대시보드는 Apple-like personal workbench를 기본 visual language로 사용한다. 구조는 `02-layout.md`를 따르고, 이 문서는 색, 밀도, 버튼 노출, light/dark theme 규칙만 정의한다.

## Direction

notegate는 개발자용 admin console처럼 보이면 안 된다. 문서와 파일을 오래 읽는 조용한 개인 작업대처럼 보여야 한다.

시각 원칙:

1. Light theme는 warm white와 cool neutral을 기본으로 한다. 종이색/베이지가 아니라 macOS 앱의 밝은 작업면에 가깝다.
2. Dark theme는 neutral graphite palette를 사용한다. 갈색/sepia dark를 피한다.
3. Primary action만 색으로 강하게 보이고, 일반 toolbar action은 hover 전까지 조용해야 한다.
4. Workbench chrome은 얇고 조용해야 한다. 영역은 굵은 border보다 미세한 separator와 surface tone으로 구분한다.
5. Empty state는 다음 행동을 안내하되 화면을 장악하지 않는다.

## Theme tokens

### Light

```text
background       #f5f5f7
surface          #ffffff
editor           #ffffff
panel            #f7f7f8
panel-strong     #ececf0
border           rgba(60,60,67,0.18)
border-strong    rgba(60,60,67,0.30)
seam             rgba(60,60,67,0.12)
selection        rgba(0,122,255,0.12)
hover            rgba(60,60,67,0.07)
text             #1d1d1f
muted            rgba(60,60,67,0.68)
faint            rgba(60,60,67,0.36)
primary          #007aff
primary-hover    #006ee6
primary-contrast #ffffff
danger           #ff3b30
success          #34c759
warning          #ff9f0a
```

### Dark

```text
background       #1c1c1e
surface          #242426
editor           #1c1c1e
panel            #262628
panel-strong     #343437
border           rgba(84,84,88,0.45)
border-strong    rgba(142,142,147,0.55)
seam             rgba(84,84,88,0.32)
selection        rgba(10,132,255,0.20)
hover            rgba(235,235,245,0.07)
text             #f5f5f7
muted            rgba(235,235,245,0.64)
faint            rgba(235,235,245,0.38)
primary          #0a84ff
primary-hover    #409cff
primary-contrast #ffffff
danger           #ff453a
success          #30d158
warning          #ffd60a
```

## Typography

- Font stack: `-apple-system`, `BlinkMacSystemFont`, `SF Pro Text`, `Inter`, `system-ui`.
- UI chrome은 12~14px를 기본으로 한다.
- 문서 본문은 16px 이상, 읽기 line-height는 1.55 이상을 기본으로 한다.
- Document heading weight는 600을 기본으로 한다.
- Headline은 크기와 여백으로 위계를 만들고 weight 700은 사용하지 않는다.

## Shape and depth

- Button/input radius: 8~10px.
- Card/panel radius: 12~16px.
- Icon/space avatar radius: 10~12px. 원형은 account/avatar처럼 의미가 분명할 때만 사용한다.
- Shadow는 거의 쓰지 않는다. 경계는 separator와 background tone으로 만든다.
- Primary action만 filled blue를 사용할 수 있다. 일반 toolbar button은 기본적으로 transparent/tonal이다.

## Noise control

항상 보이면 안 되는 action:

- Space rename
- Space delete
- Reset dev key
- Node move/delete
- Metadata edit

이 action들은 context menu, management surface, 또는 선택된 node의 inspector 안에 둔다. `TitleBar`에는 layout control과 theme control만 둔다.

## Region visual rules

### TitleBar

- 제품명과 현재 context만 짧게 보여준다.
- 위험 action을 노출하지 않는다.
- 가운데는 비워 둔다.
- 오른쪽은 layout toggle, theme toggle 같은 전역 UI action만 둔다.
- Toolbar button은 macOS toolbar처럼 조용해야 한다. 기본 상태에서는 filled button처럼 보이면 안 된다.

### ActivityRail

- Space 전환만 강하게 보인다.
- Space 추가 버튼은 Space 목록 바로 아래에 둔다.
- Settings는 하단 고정이다.
- 선택된 Space는 부드러운 tint 또는 짧은 active indicator로 표시한다. idle item마다 강한 border를 주지 않는다.
- Sidebar surface는 EditorArea보다 살짝 낮은 contrast를 유지한다. macOS source list처럼 콘텐츠보다 뒤로 물러나 보여야 한다.

### PrimarySidebar

- Header는 active space name과 `Create`/`Manage` 같은 compact action만 둔다.
- `TreeSection`과 `RecentSection`은 각각 독립 스크롤이다.
- Source-list 느낌을 우선한다: selected row는 subtle fill, hover는 더 약한 fill, 행마다 border를 만들지 않는다.
- Recent 오류는 빨간 에러 블록이 아니라 muted fallback으로 처리한다.

### EditorArea

- 선택 전 empty state는 조용하게 표시하고 `New text`, `Upload file`, `Create folder` CTA를 제공한다.
- Text는 기본 preview mode다.
- Edit mode는 같은 group 안에서 전환한다.
- Node 위험 action은 header에 항상 노출하지 않고 compact menu에 둔다.
- 문서 면은 사이드바보다 더 깨끗해야 한다. 본문 주변에 불필요한 카드/프레임을 만들지 않는다.
- Light theme의 EditorArea는 white reading surface를 사용한다. Dark theme의 EditorArea는 window background와 같은 graphite를 유지한다.
- Editor header는 toolbar처럼 낮고 조용해야 한다. 문서 내용보다 버튼/헤더가 먼저 보이면 안 된다.

### AuxiliarySidebar

- 선택된 node가 없으면 기본으로 숨긴다.
- 선택된 node가 있을 때 Inspector를 보여준다.
- Inspector는 card wall이 아니라 grouped list처럼 보여야 한다. 여러 카드가 같은 세기로 경쟁하면 안 된다.

## Light/dark behavior

- Theme는 사용자 local UI state다.
- 기본값은 system preference를 따를 수 있다.
- 사용자가 바꾸면 local storage에 저장한다.
- Theme 변경은 backend state를 변경하지 않는다.
- Dark theme는 brown/sepia가 아니라 neutral graphite여야 한다.
