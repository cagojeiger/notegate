# UI visual style

notegate 대시보드는 warm neutral workbench를 기본 visual language로 사용한다. 구조는 `02-layout.md`를 따르고, 이 문서는 색, 밀도, 버튼 노출, light/dark theme 규칙만 정의한다.

## Direction

notegate는 개발자용 admin console처럼 보이면 안 된다. 문서와 파일을 오래 읽는 개인 작업대처럼 보여야 한다.

시각 원칙:

1. Light theme는 warm cream background를 기본으로 한다.
2. Dark theme도 같은 warm neutral hue를 유지하고 차갑고 푸른 IDE palette를 피한다.
3. Primary action만 강하게 보이고, 위험/관리 action은 평소에 숨긴다.
4. Workbench chrome은 얇고 조용해야 한다.
5. Empty state는 다음 행동을 안내하되 화면을 장악하지 않는다.

## Theme tokens

### Light

```text
background       #f7f4ed
surface          #fcfbf8
panel            #f2efe7
panel-strong     #e9e4d8
border           #e3ded2
border-strong    rgba(28,28,28,0.22)
text             #1c1c1c
muted            rgba(28,28,28,0.58)
faint            rgba(28,28,28,0.36)
primary          #1c1c1c
primary-contrast #fcfbf8
danger           #b42318
success          #147a3d
warning          #a15c07
```

### Dark

```text
background       #171512
surface          #1f1c17
panel            #27231d
panel-strong     #332d25
border           #3b352c
border-strong    rgba(247,244,237,0.24)
text             #f4efe6
muted            rgba(244,239,230,0.62)
faint            rgba(244,239,230,0.38)
primary          #f4efe6
primary-contrast #171512
danger           #ff8a7a
success          #7bcf9b
warning          #f2b766
```

## Typography

- Font stack: `Camera Plain Variable`, `ui-sans-serif`, `system-ui`.
- Body/UI weight: 400~500.
- Document heading weight: 600.
- 대시보드 chrome은 12~14px를 기본으로 하고, 문서 본문은 16px 이상을 기본으로 한다.
- Headline은 크기로 위계를 만들고 weight 700은 사용하지 않는다.

## Shape and depth

- Button/input radius: 6~8px.
- Card/panel radius: 12~16px.
- Icon/space avatar radius: 9999px 또는 12px.
- Shadow는 거의 쓰지 않는다. 경계는 border와 background tone으로 만든다.
- Primary dark/light action만 inset shadow를 사용할 수 있다.

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

### ActivityRail

- Space 전환만 강하게 보인다.
- Space 추가 버튼은 Space 목록 바로 아래에 둔다.
- Settings는 하단 고정이다.

### PrimarySidebar

- Header는 active space name과 `Create`/`Manage` 같은 compact action만 둔다.
- `TreeSection`과 `RecentSection`은 각각 독립 스크롤이다.
- Recent 오류는 빨간 에러 블록이 아니라 muted fallback으로 처리한다.

### EditorArea

- 선택 전 empty state는 조용하게 표시하고 `New text`, `Upload file`, `Create folder` CTA를 제공한다.
- Text는 기본 preview mode다.
- Edit mode는 같은 group 안에서 전환한다.
- Node 위험 action은 header에 항상 노출하지 않고 compact menu에 둔다.

### AuxiliarySidebar

- 선택된 node가 없으면 기본으로 숨긴다.
- 선택된 node가 있을 때 Inspector를 보여준다.

## Light/dark behavior

- Theme는 사용자 local UI state다.
- 기본값은 system preference를 따를 수 있다.
- 사용자가 바꾸면 local storage에 저장한다.
- Theme 변경은 backend state를 변경하지 않는다.
