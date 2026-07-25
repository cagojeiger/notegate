# ADR 0006: User MCP 범위와 탐색 고정 분리

## 배경

Space를 Workbench 탐색 영역에 고정하는 선호와 User MCP에 공개하는 권한은 목적이 다르다. 하나의 Pin 상태가 두 역할을 맡으면 탐색 UI를 정리하는 동작이 MCP 접근 권한까지 바꿀 수 있다. Agent 접근은 별도의 명시적 연결 모델을 사용한다.

## 결정

Space에 서로 독립적인 두 상태를 둔다.

- `navigation_pinned`: 데스크톱 rail과 모바일 Space 전환 목록에 계속 표시한다.
- `user_mcp_enabled`: owner user의 MCP 목록과 접근 범위에 포함한다.

REST는 owner가 소유한 모든 live Space를 두 상태와 함께 반환한다. User MCP는 `user_mcp_enabled`가 활성화된 Space만 조회하고 접근한다. Agent MCP는 이 값을 무시하고 명시적으로 연결된 Space만 접근한다.

새 Space는 탐색 영역에 고정하고 User MCP에는 노출하지 않는다. 기존 Space는 이전 Pin 상태를 두 상태에 각각 복사해 기존 탐색 표시와 MCP 접근을 보존한다.

User MCP에서 비활성화된 Space와 존재하지 않는 Space는 같은 not-found 응답을 사용한다. 이 권한은 목록뿐 아니라 target 해석과 진행 중 transfer의 후속 작업에도 적용한다.

## 결과

- Space Library 카드에서 탐색 고정 상태를 빠르게 변경한다.
- Space Inspector에서 탐색 고정, User MCP 접근, 새 항목 기본 정책을 각각 관리한다.
- owner는 탐색에 고정하지 않은 Space도 Library에서 열 수 있다. 활성화되어도 탐색 영역에는 추가하지 않으며, 현재 Space는 Title Bar에서 확인한다.
- 탐색 고정 변경은 User MCP 권한을 바꾸지 않는다.
- User MCP 비활성화는 이후 MCP 작업을 즉시 차단한다. 이미 발급된 presigned URL은 기존의 짧은 만료 시점까지 유효하다.
- Agent 접근은 계속 Space connection 권한만 따른다.
