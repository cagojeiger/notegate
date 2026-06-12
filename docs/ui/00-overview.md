# UI design overview

`docs/ui`는 notegate 대시보드 UI의 용어, 레이아웃, 정보 구조, 사용자 흐름을 정의한다.

디자인 문서는 다음 순서로 작성한다.

```text
00-overview.md      전체 방향과 문서 순서
01-glossary.md      UI 표준 용어
02-layout.md        AppShell/Workbench 레이아웃
03-information.md   영역별 정보 배치
04-flows.md         핵심 사용자 흐름
06-visual.md        visual style guideline
```

## Design order

처음부터 시각 스타일을 만들지 않는다. 다음 순서로 결정한다.

```text
용어 -> 레이아웃 -> 정보 구조 -> 사용자 흐름 -> visual style
```

## Current baseline

notegate는 인증 화면과 대시보드 화면을 분리한다. 대시보드는 workbench형 UI를 사용한다.

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

기본 철학:

- Desktop은 full workbench를 보여준다.
- Mobile은 `EditorArea` 중심의 단일 작업 화면으로 표현한다.
- Layout role은 유지하고, viewport별 presentation만 바꾼다.

- Login은 `AppShell` 내부 route가 아니라 `AuthScreen`에서 처리한다.
