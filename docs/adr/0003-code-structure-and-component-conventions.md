# ADR 0003: 코드 구조와 컴포넌트 컨벤션

## 상태

채택됨

## 맥락

notegate는 개인용 AI-native 노트 서비스다. 백엔드는 `core · model · db · service · api`
크레이트로 구성한다. 초기 구조에는 store trait 기반 DIP, 도메인별 과분리, 표면별 파일 조직
불일치가 섞여 있어 탐색 비용이 컸다.

현재 정리 방향은 다음 원칙을 따른다.

- **model**: 순수 데이터 타입만 둔다. 여러 레이어가 함께 쓰는 command/view/cursor DTO도 여기에 둔다.
- **db**: Postgres repo와 SQL/transaction/CTE/제약 재검증을 담당한다.
- **service**: 비즈니스 규칙, 권한 체크, validation, orchestration을 담당하고 concrete repo를 직접 사용한다.
- **api**: REST/MCP/auth/OpenAPI/transport DTO/error mapping을 담당한다.

## 결정

### 1. store trait 기반 DIP를 제거하고 concrete repo를 직접 사용한다

단일 production 구현만 있는 `FilesStore`, `WorkspaceStore`, `AgentStore`, `AccessStore`,
`AccountStore`, `UserStore`, `AgentAuthStore`, `SearchStore`는 유지하지 않는다.

- service는 `notegate-db`의 concrete repo를 직접 소유한다.
- db는 service를 의존하지 않는다.
- 다형성이 실제로 필요한 request-time 인증 seam인 `CallerResolver`만 object-safe trait으로 유지한다.
- mock store 기반 테스트보다 Postgres 통합 테스트와 순수 함수 테스트를 우선한다.

의존 방향은 다음과 같다.

```text
api ──▶ service ──▶ db ──▶ model ──▶ core
 │        │          │
 └────────┴──────────┘  (api는 조립을 위해 db/model도 직접 참조 가능)
```

### 2. service 도메인 모듈은 관심사 기준으로만 분리한다

- 각 도메인은 `service/src/<domain>/` 또는 단일 `mod.rs`로 시작한다.
- 명확한 관심사(순수 validation, patch engine, cursor, policy, content metrics 등)가 있을 때만 파일을 나눈다.
- 단순히 메서드 수가 많다는 이유만으로 store trait/mock 구조를 만들지 않는다.

### 3. db repo 파일명은 `<domain>_repo.rs`로 통일한다

구조체 `XxxRepo`에 대응하는 파일은 모두 `_repo` 접미사를 쓴다.
`files_repo.rs`는 같은 크레이트의 `files/` 내부 SQL helper 디렉터리와 구분하기 위해서도 이 규칙이 자연스럽다.

### 4. API 표면은 표면별 사용성에 맞춰 조직한다

- REST는 화면 렌더링과 리소스 중심 API이므로 리소스별 파일로 둔다.
- MCP는 LLM/CLI 친화 도구이므로 `identity · workspaces · files · search` 카테고리별 파일로 둔다.
- REST/MCP 모두 같은 service를 호출하지만, transport DTO와 schema는 api 레이어가 책임진다.

### 5. 공통 헬퍼는 실제 중복이 있을 때만 모은다

- 표면 내부 공유 헬퍼는 `support.rs`에 둔다.
- 경로/워크스페이스 해석처럼 MCP 전용 의미가 강한 로직은 `mcp/tools/resolve.rs`처럼 별도 파일로 둔다.
- 크레이트 전역 공유(예: pagination/page/error mapping)는 크레이트 루트에 둔다.

## 근거

- trait은 실제 다형성이나 교체 가능성이 있을 때만 비용을 정당화한다.
- production 구현이 하나뿐인 store trait은 concrete repo의 그림자 인터페이스가 되기 쉽고,
  제네릭·mock·impl Future 보일러플레이트를 늘린다.
- DB 제약, trigger, recursive CTE, transaction race 방어는 mock이 정확히 흉내내기 어렵다.
  따라서 핵심 lifecycle/search/permission은 실제 Postgres 통합 테스트가 더 신뢰도 높다.
- model에 shared DTO를 둬서 db와 service가 같은 데이터 계약을 쓰되, HTTP/MCP schema 세부사항은 api가 갖는다.

## 결과

- service store trait과 mock store 테스트는 제거한다.
- service 순수 함수 테스트(content/patch/policy/validation/cursor)는 유지한다.
- files/search/permissions 같은 end-to-end 성격의 테스트는 service 통합 테스트로 둔다.
- repo 자체 동작은 db 통합 테스트로 둔다.
