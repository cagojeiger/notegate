# Settings 상태 전환

Settings의 표시 상태와 기능별 작업 상태는 독립적이다. 이 문서는 기존 React state와 query mutation의 동작 계약을 FSM으로 표현한다. 별도 런타임 상태 머신이나 서버 API는 도입하지 않는다.

## 1. Inspector 표시 FSM

```mermaid
stateDiagram-v2
    [*] --> Unavailable
    Unavailable --> Hidden: Inspector 제공 / 닫힘 선호
    Unavailable --> Docked: Inspector 제공 / desktop 열림 선호
    Unavailable --> Sheet: Inspector 제공 / mobile 열림 선호
    Hidden --> Docked: 열기 또는 resize / desktop 열림 선호
    Hidden --> Sheet: 열기 또는 resize / mobile 열림 선호
    Docked --> Hidden: 닫기 또는 상단 toggle
    Sheet --> Hidden: Close, Escape 또는 backdrop
    Docked --> Sheet: mobile 전환 / mobile 열림 선호
    Sheet --> Docked: desktop 전환 / desktop 열림 선호
    Docked --> Hidden: mobile 전환 / mobile 닫힘 선호
    Sheet --> Hidden: desktop 전환 / desktop 닫힘 선호
    Hidden --> Unavailable: Inspector 미제공 화면
    Docked --> Unavailable: Inspector 미제공 화면
    Sheet --> Unavailable: Inspector 미제공 화면
```

표시 상태는 `available`, 현재 viewport의 `open`, `isMobile`에서 계산한다. 사용 가능한 화면으로 돌아올 때 desktop의 열림 선호는 다시 적용할 수 있다. Mobile에서 Inspector를 제공하지 않는 화면으로 이동하면 열림 상태를 해제한다.

- Workbench는 표시 위치·열림 상태와 portal container를 소유한다.
- Feature는 선택 대상·확인·요청 결과를 소유한다. Portal의 unmount를 feature의 unmount로 간주하지 않는다.
- Inspector가 닫혀도 본문의 탐색·추가·필터는 사용할 수 있다. 진행 중 요청 때문에 일시적으로 비활성화하는 경우는 다음 FSM을 따른다.

## 2. 파괴적 작업 FSM

Account의 브라우저 로그아웃, Connections의 연결 해제, Agents의 삭제, Trash의 영구 삭제에 같은 닫기 규칙을 적용한다.

```mermaid
stateDiagram-v2
    [*] --> Browsing
    Browsing --> Confirming: 작업 선택 / Inspector 표시 중
    Confirming --> Browsing: Cancel 또는 Inspector 숨김
    Confirming --> Pending: 명시적 Confirm / 요청 1회
    Pending --> Pending: Inspector 닫기 또는 재열기 / 재요청 없음
    Pending --> Feedback: 성공
    Pending --> Confirming: 실패 / Inspector 표시 중, 재확인 가능
    Pending --> Feedback: 실패 / Inspector 숨김 또는 대상 변경
    Feedback --> Browsing: 다른 대상 선택
    Feedback --> Confirming: 사용자가 작업을 다시 선택
```

| 전환·경계 | 반드시 지킬 동작 |
| --- | --- |
| 확인 중 → Inspector 닫기 | 미확정 작업을 취소한다. 목록과 새로고침의 확인 상태 잠금을 해제한다. 요청은 보내지 않는다. |
| 요청 중 → Inspector 닫기 | 이미 보낸 요청을 취소·재전송하지 않는다. 요청 때문에 잠근 조작은 응답을 처리할 때까지 잠근다. |
| 닫힌 상태에서 응답 도착 | 성공·실패 결과를 보존하고 미확정 확인 상태를 해제한다. 다시 열 때 오래된 Confirm을 복구하지 않는다. |
| 표시 중 실패 | 사용자가 확인 후 재시도할 수 있다. Trash의 404·409는 확인을 해제하고 목록을 새로 조회한다. |
| Cancel | 실행 버튼으로 focus를 돌린다. Inspector 닫기는 sheet의 opener 복귀 규칙을 따른다. |
| viewport 변경 | 변경 후에도 Inspector가 표시된다면 확인 상태를 유지한다. 숨겨진다면 닫기 규칙을 적용한다. |
| tab 변경·Settings 종료 | UI의 확인 상태를 버린다. 서버로 보낸 작업을 되돌렸다고 표시하지 않는다. |

서버 mutation이 진행 중인지 여부는 query mutation이 소유한다. 닫기 처리는 미확정 UI state만 정리하며 mutation의 reset이나 취소를 호출하지 않는다. 성공 후 목록 갱신과 실패 처리는 기존 mutation callback이 수행한다. Trash의 삭제 성공은 정리 완료가 아닌 `Deletion queued`다.

전환 처리 후의 불변 조건:

- Inspector 숨김 + 요청 없음 → 확인 상태 없음.
- 요청 중 → Inspector 표시 여부와 무관하게 요청으로 잠근 조작을 유지.
- 닫기·재열기·resize → mutation 전송 횟수 증가 없음.
- 재시도 → 사용자의 명시적 Confirm 필요.

## 3. Mobile sheet의 focus FSM

```mermaid
stateDiagram-v2
    Opener --> Close: sheet 열기
    Close --> Input: Tab
    Input --> Response: Tab
    Response --> Close: Tab
    Close --> Response: Shift+Tab
    Response --> Input: Shift+Tab
    Input --> Close: Shift+Tab
    Input --> Input: Enter / disclosure 토글
    Response --> Response: Enter / disclosure 토글
    Close --> Opener: Escape 또는 닫기
    Input --> Opener: Escape
    Response --> Opener: Escape
```

이 도식은 다른 focusable content가 없는 호출 상세의 기본 순서다. Native `summary`도 Tab 순환에 포함한다. 명시적 autofocus가 있으면 유지하며, sheet 종료 중 새 화면으로 옮겨진 focus를 오래된 opener로 덮어쓰지 않는다.

## 전환 검증

- [`settings-transitions.spec.ts`](../../frontend/web/e2e/settings-transitions.spec.ts): desktop/mobile에서 4개 기능의 닫기 경로·재열기·resize, desktop tab 변경, 미전송 보장, 요청 중 재열기·닫기와 이후 성공·실패의 중복 전송 방지.
- [`invocation-history.spec.ts`](../../frontend/web/e2e/invocation-history.spec.ts): 실제 mobile History Inspector의 summary 접근·Enter·Tab 양방향 순환과 기존 스크롤·선택 복원.
- [`Modal.test.tsx`](../../frontend/web/src/shared/ui/Modal.test.tsx): 공통 focus 순환, disabled/tabIndex 제외, autofocus와 opener 복귀.
- 기존 [`control-interactions.spec.ts`](../../frontend/web/e2e/control-interactions.spec.ts), [`account-access.spec.ts`](../../frontend/web/e2e/account-access.spec.ts), [`trash.spec.ts`](../../frontend/web/e2e/trash.spec.ts): tab 탐색·선택·취소·결과 안내·pagination 보존.

이 검증은 명시한 UI 전환을 실행하는 회귀 테스트다. 가능한 모든 비동기 interleaving에 대한 형식 검증을 의미하지 않는다.
