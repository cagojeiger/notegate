# 디자인 원칙

상태: Active
마지막 검토: 2026-08-19

## 문서 경계

- 이 문서는 제품의 시각·상호작용 원칙과 사용자에게 쓰는 언어를 정의한다.
- 구체적인 화면 구조와 상태 흐름은 [`docs/ui`](docs/ui/README.md)가 정의한다.
- API, 보안, 저장소와 성능 계약은 [`docs/spec`](docs/spec/api.md)이 정의한다.
- 실제 동작, 상수와 디자인 토큰의 정본은 코드다. 문서와 코드가 다르면 현재 코드를 확인하고 같은 변경에서 문서를 맞춘다.

## 브랜드

- 성격: 차분하고 정확하며 신뢰할 수 있는 작업 도구.
- 제품명: 항상 `NoteGate`로 쓴다.
- 심볼: 열린 게이트와 세 노드 디렉터리 트리를 기본 표식으로 사용한다. 32 px 미만에서는 앱 아이콘, 그 이상에서는 전체 심볼이나 로고 조합을 사용한다.
- 신뢰 신호: 명확한 Google 로그인, 읽을 수 있는 상태, 절제된 색상, 구체적인 보안·복구 문구.
- 피할 것: 장식용 그라디언트, 색상만으로 전달하는 상태, 임의의 문자 배지, 혼합된 아이콘 스타일, 중첩 카드 남용.

## 제품 목표

- 노트와 파일을 오래 읽어도 피로하지 않게 한다.
- 파일 트리와 게이트라는 제품 구조를 처음 화면에서 인식할 수 있게 한다.
- 인증, 동기화, 업로드와 시스템 상태를 모호하지 않게 표현한다.
- 운영 화면은 서비스 상태에서 원인 후보로 자연스럽게 좁혀 갈 수 있어야 한다.

주 사용자는 개인 노트와 Agent 접근을 관리하는 사용자, 로컬 서비스와 검색 성능을 점검하는 개발자·운영자다.

## 상호작용 원칙

- 읽기 우선: 편집기와 문서 내용이 주변 UI보다 높은 시각적 우선순위를 가진다.
- 점진적 공개: 자주 쓰지 않는 설정과 설명은 관련 Inspector나 도움말에 둔다.
- 의미 보존: 상태는 색상과 함께 텍스트, 아이콘 또는 형태로 표현한다.
- 일관된 도구 언어: 브랜드 자산은 제품 식별에만 쓰고, 동작과 객체에는 Lucide 아이콘을 사용한다.
- 안정적인 배치: 동적 상태가 나타나도 주요 탐색과 편집 영역이 불필요하게 이동하지 않게 한다.
- 서버 상태 우선: 유지보수 동작의 실행 가능 여부는 서버가 제공하는 pending·available 시각을 기준으로 하며, 로컬 mutation 상태는 조회가 갱신될 때까지의 즉각적인 피드백에만 사용한다.
- 대안 제공: 드래그, hover 또는 색상만으로 수행하거나 이해해야 하는 기능을 만들지 않는다.

## 시각 언어

- 방향: `Compact Workbench + Calm Reader`. 탐색·상태·도구 chrome은 개발 도구처럼 조밀하게, 문서 읽기 영역은 여유롭게 유지한다.
- 색상: `Paired Violet Neutral + Muted Violet`을 따른다. Windows의 GitLab Web IDE가 사용하는 VS Code식 surface 계층을 참고해 Light는 lavender paper chrome보다 editor를 단계적으로 밝게, Dark는 smoky graphite chrome보다 surface와 editor를 한 단계 밝게 표현한다. Muted Violet은 링크·포커스·선택·기본 동작만 담당하고 화면의 약 5% 안쪽으로 제한한다. hover는 neutral, selected surface는 옅은 violet로 구분한다. 초록색은 정상, 주황색은 주의, 빨간색은 실패, 파란색은 정보 상태에만 쓴다.
- Workbench 글꼴: title bar, Files/Recent, editor tab, Inspector, modal과 status bar를 포함한 시스템 UI는 VS Code와 같은 운영체제별 UI stack을 사용한다. 기본 크기는 13 px, line-height는 18 px다. macOS와 Windows의 한국어 글꼴은 각각 Apple SD Gothic Neo와 Malgun Gothic을 우선 fallback으로 둔다.
- Files/Recent section label은 대문자 변환이나 별도 자간 없이 title case 13 px Medium으로 표시한다.
- Reading 글꼴: Markdown과 일반 텍스트 본문은 기존 운영체제 UI stack을 유지한다. Markdown은 기존 16 px/1.7 line-height와 문서 간격을 그대로 유지하고, 코드·경로·식별자는 기존 monospace stack을 사용한다.
- 간격: 4 px 리듬을 기준으로 desktop chrome은 36 px header, 28 px control, 26 px row, 22 px status bar를 기본으로 한다. 모바일의 toolbar와 독립 control은 44 px를 유지하되, 전체 폭이 하나의 target인 Files/Recent 행은 36 px로 조밀하게 표시한다.
- Recent 목록 보기는 파일명과 경로를 한 항목으로 묶기 위해 행 안쪽 상하 2 px, 두 줄 사이 2 px, 항목 사이 2 px를 사용한다. 한 줄짜리 압축 보기와 Files 행의 밀도는 유지한다.
- 선택 상태: hover, selected, inspected, active 상태가 행의 padding, 높이, 글자 굵기를 바꾸지 않아야 한다. Files/Recent 행은 장식용 side rail 없이 배경만 바꾸고, `aria-current`로 열린 항목을 표현한다.
- 형태: Workbench의 붙어 있는 control과 row는 4 px, section surface는 6 px radius를 기본으로 한다. Inspector는 중첩 카드 대신 얇은 seam으로 나뉜 flat section을 사용한다. 큰 radius와 그림자는 modal과 떠 있는 surface에만 사용한다.
- 아이콘: 기능 아이콘은 원칙적으로 16 px Lucide와 1.75 px stroke를 사용한다.
- Section header의 동작은 메타데이터 또는 `xs` ghost control로 제한한다. 인덱스 재구축·사용량 재계산 같은 유지보수 동작은 강한 외곽선으로 제목과 경쟁하지 않으며, 섹션에 주요 데이터가 있으면 데이터 다음에 배치한다. 모바일에서는 시각적 밀도와 별개로 44 px touch target을 유지한다.
- Workbench의 텍스트 버튼은 `xs`, `sm`, `md` 크기에 관계없이 13 px 기본 글자 크기를 사용한다. size variant는 높이와 좌우 padding만 바꾸며, 버튼 간 정보 위계는 variant·색·배치로 표현한다.
- 브랜드: Workbench 안의 앱 아이콘은 동일한 도형을 유지하면서 Light와 Dark surface에 맞는 전용 색상 변형을 사용한다. 워드마크, favicon과 설치용 PWA 아이콘은 테마와 무관한 고정 자산으로 유지한다.
- 움직임: 짧은 색상·투명도 전환만 사용하고 `prefers-reduced-motion`을 존중한다.

세부 layout, 반응형 정책과 component 배치는 [`docs/ui/01-layout.md`](docs/ui/01-layout.md)를 따른다.

## 접근성

- 목표는 WCAG 2.2 AA다.
- 일반 텍스트는 4.5:1, 큰 텍스트와 의미 있는 UI 경계는 3:1 대비를 유지한다.
- 링크, button, field, tab과 명시적 focus target에는 보이는 focus outline을 제공한다.
- icon-only control은 문맥을 포함한 접근성 이름을 가진다.
- 상태 변경과 비동기 결과는 적절한 live region으로 전달한다.
- drag가 가능한 항목에는 keyboard와 touch로 사용할 수 있는 대체 동작을 제공한다.
- desktop의 시각 밀도와 별개로 coarse pointer와 모바일에서는 interactive target을 44 px 이상 유지한다.

구체적인 keyboard, tab, resize와 반응형 동작은 [`docs/ui`](docs/ui/README.md)에만 기록한다.

## 사용자 문구

- 짧고 직접적이며 차분하게 쓴다.
- 사용자가 수행한 동작과 다음 행동을 설명하고 내부 인증·저장 구현은 노출하지 않는다.
- 사용자 화면의 content kind에는 `Document`, `Folder`, `File`을 사용한다. 저장 용량과 암호화 정책처럼 본문 저장 형식을 가리킬 때는 `Text`를 사용한다.
- 주요 화면 이름은 `Space`, `Files`, `Recent`, `Inspector`, `Details`, `Outline`로 통일한다.
- `node`는 API와 구현 용어다. 사용자에게는 구체적인 content kind를 사용하고, 종류를 알 수 없을 때만 `item`을 사용한다.
- 지속적인 설명문보다 명확한 배치, label과 문맥 도움말을 우선한다.

## 구현 경계

- React, TypeScript, Tailwind utility와 기존 `--ng-*` CSS custom property 체계를 유지한다.
- token의 정본은 [`frontend/web/src/design/theme.css`](frontend/web/src/design/theme.css)다.
- 브랜드 기준색은 Ink Slate `#202833`, Light paper `#F1F1FF`, Light accent `#6761A8`, Dark background `#222024`, Dark accent `#AAA5E3`이다. 일반·보조 텍스트와 의미 있는 경계는 이 기준색보다 WCAG 2.2 AA 충족을 우선해 조정할 수 있다.
- 기존 Workbench의 정보 구조와 배치(`Activity Rail → Files/Recent → Editor Groups → Details/Outline → Status Bar`)를 유지한다. 좌우 panel resize·폭 저장, 최대 3개 editor group과 mobile overlay 동작도 시각 갱신 때문에 바꾸지 않는다.
- 새 theme system, feature별 raw color, 외부 font CDN 또는 별도 icon dependency를 추가하지 않는다. Workbench UI는 플랫폼 기본 글꼴을 따르고 Reading 영역은 `--font-reading` 경계를 유지한다.
- Workbench density와 radius는 `theme.css`의 semantic token을 사용하고 Markdown의 typography/spacing을 이 scale에 결합하지 않는다.
- 시각 변경은 1440×900 desktop과 390×844 mobile에서 light/dark 모두 확인한다.
- 상세한 상태 소유권, 캐시, preview, recording과 검증 규칙은 [`docs/ui/02-data-and-flows.md`](docs/ui/02-data-and-flows.md)와 [`docs/ui/03-implementation.md`](docs/ui/03-implementation.md)를 따른다.
- 운영 dashboard의 지표와 label 정책은 [`docs/spec/observability.md`](docs/spec/observability.md)가 소유한다.

## 이번 갱신의 비범위

- native 운영체제 menu chrome을 모방하지 않는다.
- 승인 시안의 panel 배치나 정보 구성을 NoteGate에 이식하지 않는다. 시안은 typography, density, surface 표현의 기준으로만 사용한다.
- 시각적 일치를 위해 기존 action, metadata 또는 status 정보를 제거하거나 새 기능을 만들지 않는다.
- Markdown과 code syntax highlighting의 고유 색상 체계는 이번 Workbench palette 변경에 결합하지 않는다.
