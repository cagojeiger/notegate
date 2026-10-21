# UI 제약

색상·치수·컴포넌트 목록과 화면별 호출 순서는 문서에 복제하지 않는다. 정본은 [theme.css](../../frontend/web/src/design/theme.css), [공통 UI](../../frontend/web/src/shared/ui), 각 feature와 browser test다. 이 문서에는 구현을 바꿔도 유지할 제품 제약만 둔다.

## 읽기와 조작

- 문서 읽기·편집을 우선한다. 창이 좁아져도 열린 문서와 editor group을 버리지 않고 보조 패널을 overlay로 전환한다. 표시 폭을 줄였다고 사용자가 저장한 선호 폭을 덮어쓰지 않는다.
- 시각 변경을 이유로 기존 동작·상태·metadata를 제거하거나 새 기능을 추가하지 않는다.
- 색상·치수는 semantic token, 반복되는 chrome은 공통 UI가 소유한다. 문서 typography는 Workbench 밀도와 독립적이다. 외부 font CDN과 별도 theme/icon 체계를 추가하지 않는다.
- Workspace와 Settings는 같은 상단 탭·본문·선택형 오른쪽 Inspector 틀을 사용한다. 탭은 현재 영역의 화면을 전환하고 Inspector는 그 화면의 선택 대상에 속한다. Mobile에서도 이 관계를 유지하되 Inspector를 아래쪽 sheet로 표시한다.
- 사용자에게 내부 `node`나 인증 구현 대신 구체적인 Document·Folder·File과 다음 행동을 보여준다. 유지보수 실행 가능 시각은 서버가 판단한다.

## 상태와 개인정보

- 서버 자원은 query cache가 소유한다. Browser workspace는 개인 기기의 best-effort 편의 상태이며 계정 데이터의 정본이나 동기화 수단이 아니다.
- Workspace에는 탐색·panel 상태와 Node snapshot만 저장한다. 문서 본문·미저장 draft·녹음 chunks·credential 원문을 local storage에 저장하지 않는다. Workspace reset은 서버의 Space·문서·파일을 삭제하지 않는다.
- 저장 실패·충돌·일시 장애를 빈 목록, 권한 없음이나 로그아웃으로 바꾸지 않는다. 충돌을 새 hash로 자동 재시도하지 않으며 늦은 응답이 현재 draft·Space를 덮어쓰지 않게 한다.
- Browser credential은 [인증 계약](../spec/rest/auth.md)을 따른다. 인증 승인·거절·연결 해제는 JavaScript 없이도 서버 폼으로 동작한다.

## 외부 콘텐츠와 녹음

- Mermaid의 외부 자원 요청은 사용자가 소스를 확인하고 `Load diagram`에 동의한 뒤 허용한다. 소스가 바뀌면 다시 동의를 받는다.
- 미리보기는 원본을 수정하지 않는다. 검증되지 않았거나 client 암호화된 파일을 추측해 디코딩하지 않고 다운로드를 제공한다. DOCX의 정확한 인쇄 배치는 원본이 정본이다.
- 녹음은 명시적 microphone 권한과 same-origin 단일 녹음 lock을 요구한다. Device 식별 정보를 보관하지 않는다. 저장 전 tab/OS 종료는 복구를 보장하지 않으며, upload 완료 전 foreground 유지가 필요하다.
- 녹음 중에는 읽기·탐색을 허용하되 저장 대상을 바꾸거나 파괴하는 조작을 막는다. 서버에 저장된 뒤의 정리는 browser 상태와 분리한다.

## 접근성과 검증

- WCAG 2.2 AA를 기준으로 한다. 색상·hover·drag만으로 상태를 전달하거나 동작하게 하지 않고 키보드·touch 대안을 제공한다.
- Icon control에 문맥 있는 이름과 보이는 focus를 제공한다. Menu의 키보드 이동·닫기·focus 복귀, 비동기 결과 안내와 reduced motion을 유지한다.
- Desktop의 시각 밀도와 mobile/coarse-pointer의 조작 영역은 별개다.
- 화면 검증은 [개발 가이드](../development.md#검증)를 따른다. Screenshot은 외관의 증거이며 동작·접근성의 대체가 아니다.

Settings의 표시·확인·요청·focus 전환 계약은 [Settings 상태 전환](settings-state-machines.md)을 따른다.
