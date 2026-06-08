# ADR 0003: 코드 구조와 컴포넌트 컨벤션

## 상태

채택됨

## 맥락

백엔드는 `core · model · service · db · api` 크레이트로 구성된다. 기능은 충실히 구현됐고
빌드/clippy는 깨끗하지만, 같은 종류의 컴포넌트가 서로 다른 방식으로 조직되어 있어
코드량 대비 탐색 비용이 크다는 문제가 제기됐다.

현재 상태에서 확인된 비일관성은 다음과 같다.

- **레이어링 역전**: 정상 기대는 `service → db`이지만, store trait(`FilesStore`,
  `SearchStore`, `AgentAuthStore` 등)과 일부 DTO(`StoredContent`, `DocumentStats`)가
  `service`에 정의되고 `db`가 이를 구현하기 때문에 실제 의존은 `db → service`다.
- **service 도메인 구성 제각각**: `files/`는 15파일로 깊게 분해(내부에 다시 `service/`
  서브모듈)되어 있는 반면 `access · agents · workspaces · search · identity`는 각각 단일
  `mod.rs`(463~735L)다. 한 크레이트에 "깊은 분해"와 "단일 파일" 두 원칙이 공존한다.
- **분할 임계값 부재**: `files`는 449L에서 파일을 쪼갰지만 `agents/mod.rs`는 735L 단일
  파일로 남아 있다. 무엇을 언제 쪼개는지 기준이 없다.
- **삼중 `service` 네이밍**: `crate service → files 모듈 → service 서브모듈`로
  `notegate_service::files::service::...` 경로가 만들어진다.
- **db repo 파일명 불일치**: `account_repo.rs · agent_repo.rs · files_repo.rs`는 `_repo`
  접미사를 쓰고 `access.rs · workspaces.rs`는 쓰지 않는다. 구조체는 모두 `XxxRepo`다.
- **api 공유 헬퍼 컨벤션 불일치**: REST는 `dto.rs` 하나, MCP는 `common.rs` + `resolve.rs`로
  공유 코드 위치 규칙이 다르다.
- **REST vs MCP 조직 원리 차이**: REST는 리소스별 묶음(`nodes.rs`에 여러 연산), MCP는 연산별
  1파일(`files_mkdir.rs` 등)이다.

이 ADR은 "정답 하나로 통일"이 목표가 아니라, 각 결정에 **명시적 규칙과 근거**를 부여해
이후 정리·리뷰·신규 코드가 같은 기준을 따르도록 하는 것이 목표다.

## 결정

### 1. 레이어링: service가 포트(port)를 소유하는 DIP를 채택한다

`db → service` 의존은 의도된 의존성 역전(Dependency Inversion)으로 **유지**한다.
store trait은 "service가 필요로 하는 영속성 계약"이므로 service에 정의하고, db가 어댑터로
구현한다.

단, 다음을 조건으로 한다.

- 포트(trait)와 포트 전용 DTO는 `service/<domain>/` 안의 명확한 위치(`store.rs` 또는
  `mod.rs` 상단)에 모은다. 영속성 모양의 타입이 service 전반에 흩어지지 않게 한다.
- trait당 프로덕션 구현이 1개뿐이고 두 번째 구현이 테스트 목(mock)에 불과하다는 점은
  알려진 비용이다. 이 DIP를 제거하고 concrete repo를 직접 쓰는 방향은 별도 ADR에서 다룬다
  (본 ADR은 현재 구조의 일관성 규칙만 확정한다).

### 2. service 도메인 모듈: "단일 `mod.rs`에서 시작, 임계값 초과 시 관심사별 분할"

- 각 도메인은 `service/src/<domain>/` 디렉터리 + `mod.rs`로 시작한다.
- `mod.rs`가 **약 400 LOC를 넘거나** 명확히 분리되는 관심사(예: 순수 검증, 입출력 DTO,
  변환 로직)가 생기면, **도메인 디렉터리 바로 아래** 평면(flat)으로 파일을 추가해 분할한다.
- 따라서 `files/`의 분할은 정당하며, **`agents/mod.rs`(735L)와 `search/mod.rs`(585L),
  `workspaces/mod.rs`(567L)는 분할 대상**으로 본다(후속 정리에서 적용).

### 3. service 내부 중첩 `service/` 서브모듈을 제거한다

`files/service/{mutate,read,range,view}`는 **`files/` 바로 아래로 평탄화**한다
(`files/mutate.rs`, `files/read.rs` …). `notegate_service::files::service::*` 같은 삼중
`service` 경로를 없앤다. 다른 도메인도 분할 시 같은 평면 규칙(결정 2)을 따른다.

### 4. db repo 파일명은 `<domain>_repo.rs`로 통일한다

구조체 `XxxRepo`에 대응하는 파일은 모두 `_repo` 접미사를 쓴다.
`access.rs → access_repo.rs`, `workspaces.rs → workspaces_repo.rs`로 맞춘다.
(`files_repo.rs`는 같은 크레이트의 `files/` 디렉터리와 충돌을 피하기 위해서도 이 규칙이
자연스럽다.)

### 5. api 공유 헬퍼 위치를 표면별로 통일한다

- **타입(DTO)**: 표면 루트의 `dto.rs`에 둔다 (`rest/dto.rs`, `mcp/tools/dto.rs`).
- **공유 로직(헬퍼)**: 표면 루트의 `support.rs`에 둔다. MCP의 기존 `common.rs` +
  `resolve.rs`는 역할이 다르면 둘로 유지하되, 단순 헬퍼 모음은 `support.rs`로 수렴한다.
- 크레이트 전역 공유(예: `page.rs`)는 크레이트 루트에 둔다(현행 유지).

### 6. REST는 리소스별, MCP는 연산별 조직을 의도된 규칙으로 명문화한다

두 표면의 입도 차이는 **버그가 아니라 표면의 본질에 따른 의도된 선택**이다.

- REST 핸들러는 **리소스별 파일**(`nodes.rs`, `documents.rs`, `workspaces.rs`)에 모은다.
- MCP는 **툴이 곧 단위**이므로 **툴별 1파일**(`files_mkdir.rs` 등)을 유지한다.
- 단, 두 표면이 같은 service를 호출하는 만큼 **공유 가능한 변환/에러 매핑은 결정 5의 공유
  위치로 끌어올린다**(중복 최소화는 별도 작업에서 다룬다).

### 7. model은 "타입당 파일, 하위 클러스터만 서브네임스페이스"

도메인 타입은 파일당 하나(`account.rs`, `node.rs` …)를 유지한다. `identity/`처럼 여러
타입이 한 개념으로 묶일 때만 서브디렉터리를 만든다(현행 유지).

### 8. 공통 헬퍼 중복은 단일 정의로 모은다

같은 동작의 헬퍼가 여러 파일에 복제된 경우 한 곳으로 모은다(예: `db`의 `map_sqlx_error`
5벌 → 라벨 인자를 받는 단일 함수, auth의 동일 `map_identity_error`/쿠키 빌더). 의미가
실제로 다른 변형(예: agent 키의 `map_identity_error`)은 별도로 둔다.

## 근거

- 컨벤션의 핵심 가치는 "정답"이 아니라 **예측 가능성**이다. 규칙이 하나면 파일 위치·이름을
  기억이 아니라 규칙으로 찾을 수 있어 탐색 비용과 체감 규모가 함께 줄어든다.
- 레이어링 역전을 "버그"가 아닌 "명시된 DIP"로 규정하면, 받아들이고 일관되게 쓸지 아니면
  별도 ADR로 걷어낼지를 분리해서 판단할 수 있다.
- 400 LOC 임계값은 절대 기준이 아니라 "분할/집중 결정을 강제로 내리게 하는 트리거"다.
  files만 쪼개고 agents는 방치되는 비대칭을 막는다.
- REST/MCP 입도 차이는 표면의 단위가 다르기 때문이며, 억지로 같게 만들면 오히려 부자연스럽다.
  대신 중복은 공유 위치 규칙(결정 5·6)으로 흡수한다.

## 결과

- 본 ADR은 **규칙만 확정**한다. 실제 파일 이동/이름 변경/분할은 후속 정리 작업에서 점진 적용하며,
  각 단계는 빌드/clippy/테스트 green을 유지한다.
- 후속 정리의 1차 적용 대상(저위험):
  - 결정 4: `access.rs → access_repo.rs`, `workspaces.rs → workspaces_repo.rs`
  - 결정 3: `files/service/*` → `files/*` 평탄화
  - 결정 8: `map_sqlx_error`/`map_identity_error`/쿠키 빌더 단일화
- 후속 정리의 2차 대상(중간 규모): 결정 2에 따른 `agents`/`search`/`workspaces` 도메인 분할,
  결정 5에 따른 api 공유 헬퍼 정리.
- 레이어링 역전 제거(store trait/제네릭 걷어내기)는 본 ADR 범위 밖이며, 채택 시 이 문서의
  결정 1을 갱신하는 별도 ADR로 다룬다.
