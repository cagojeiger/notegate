# 디자인 원칙

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
- 대안 제공: 드래그, hover 또는 색상만으로 수행하거나 이해해야 하는 기능을 만들지 않는다.

## 시각 언어

- 색상: `#17212b`와 `#f7f9fb`를 기본 중성 축으로 사용한다. 파란색은 링크, 선택, 포커스와 기본 동작에 사용한다. 초록색은 정상, 주황색은 주의, 빨간색은 실패를 뜻한다.
- 글꼴: 운영체제 기본 UI 글꼴을 chrome과 본문에 사용하고, 코드·경로·식별자에는 기본 monospace를 사용한다. 웹 폰트는 불러오지 않는다.
- 간격: 4 px 리듬을 기준으로 control 간격은 8–12 px, component 간격은 16–24 px를 사용한다.
- 형태: control은 8–10 px, panel은 12–16 px radius를 사용한다. 그림자는 modal과 떠 있는 surface에만 사용한다.
- 아이콘: 기능 아이콘은 원칙적으로 16 px Lucide와 1.75 px stroke를 사용한다.
- 움직임: 짧은 색상·투명도 전환만 사용하고 `prefers-reduced-motion`을 존중한다.

세부 layout, 반응형 정책과 component 배치는 [`docs/ui/01-layout.md`](docs/ui/01-layout.md)를 따른다.

## 접근성

- 목표는 WCAG 2.2 AA다.
- 일반 텍스트는 4.5:1, 큰 텍스트와 의미 있는 UI 경계는 3:1 대비를 유지한다.
- 링크, button, field, tab과 명시적 focus target에는 보이는 focus outline을 제공한다.
- icon-only control은 문맥을 포함한 접근성 이름을 가진다.
- 상태 변경과 비동기 결과는 적절한 live region으로 전달한다.
- drag가 가능한 항목에는 keyboard와 touch로 사용할 수 있는 대체 동작을 제공한다.

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
- 새 theme system, feature별 raw color, web font 또는 별도 icon dependency를 추가하지 않는다.
- 상세한 상태 소유권, 캐시, preview, recording과 검증 규칙은 [`docs/ui/02-data-and-flows.md`](docs/ui/02-data-and-flows.md)와 [`docs/ui/03-implementation.md`](docs/ui/03-implementation.md)를 따른다.
- 운영 dashboard의 지표와 label 정책은 [`docs/spec/observability.md`](docs/spec/observability.md)가 소유한다.
