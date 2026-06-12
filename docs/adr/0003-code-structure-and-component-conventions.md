# ADR 0003: 코드 구조와 테스트 전략

## 상태

채택됨

## 맥락

notegate 백엔드는 `core · model · db · service · api` 크레이트로 구성한다. 구조와 테스트 전략은
PostgreSQL 중심의 단일 제품 백엔드라는 전제에 맞춘다.

두 가지 사실이 구조와 테스트 방향을 결정한다.

- **DB는 PostgreSQL로 고정한다.** 다른 DB로 교체할 계획이 없다. 따라서 repository를 추상화해
  교체 가능하게 둘 이유가 없다.
- **DB의 정확성은 SQL 제약·트리거·recursive CTE·트랜잭션 경쟁·UNIQUE에 있다.** 이 부분은 mock이
  충실히 흉내낼 수 없고, mock 기반 테스트는 거짓 안심을 준다.

## 결정

### 1. mock을 최소화한다 — store-trait DIP를 두지 않는다

- 단일 production 구현뿐인 store trait(`FilesStore`, `SpaceStore`, `AccountStore` 등)을 두지
  않는다. service는 `notegate-db`의 concrete repo를 직접 소유·사용한다.
- 다형성이 실제로 필요한 request-time 인증 seam(`CallerResolver`)만 object-safe trait으로 유지한다.
- 다형성이 필요한 지점은 해당 boundary에 좁게 둔다. 선제적 store 추상화는 두지 않는다.

### 2. 테스트 전략 — 순수 로직은 유닛테스트, 나머지는 통합테스트

mock을 안 쓰는 대신 테스트를 두 층으로 나눈다.

- **순수 로직**(validation, patch engine, cursor, pagination, policy, content metrics 등)은
  DB 없는 **순수 함수 유닛테스트**로 빠르게 검증한다.
- **DB 결합 동작**(repo 동작, 권한 판정, lifecycle, search, 제약·트랜잭션·동시성)은 **실제 Postgres
  통합테스트**로 검증한다.
- mock store 기반 테스트는 쓰지 않는다. 정확성이 SQL/제약에 있어 mock으로는 검증되지 않는다.
- 테스트 속도는 mock 대체가 아니라 **테스트 인프라**로 관리한다.

### 3. 의존 방향

```text
api ──▶ service ──▶ db ──▶ model ──▶ core
 │        │          │
 └────────┴──────────┘  (api는 조립을 위해 db/model도 직접 참조 가능)
```

- db는 service를 의존하지 않는다.
- model은 여러 레이어가 함께 쓰는 순수 데이터 타입과 command/view/cursor DTO를 둔다.
- service는 비즈니스 규칙·권한 체크·validation·orchestration을 담당한다.
- api는 REST/MCP/auth/OpenAPI/transport DTO/error mapping을 담당한다.

### 4. 구조 컨벤션 (보조)

- service 도메인은 관심사 기준으로만 분리한다. 단순히 메서드 수가 많다는 이유로 trait/mock 구조를
  만들지 않는다.
- db repo 파일명은 구조체 `XxxRepo`에 대응해 `<domain>_repo.rs`로 통일한다.
- API 표면은 표면별 사용성에 맞춰 조직한다(REST는 리소스별, MCP는 identity와
  read/search/write/manage 행동 영역별). transport DTO/schema는 api 레이어가 책임진다.
- 공통 헬퍼는 실제 중복이 있을 때만 모은다(표면 내부 공유는 `support.rs`, 크레이트 전역 공유는
  크레이트 루트).

## 근거

- trait은 실제 다형성이나 교체 가능성이 있을 때만 비용을 정당화한다. PostgreSQL 고정 + 단일 구현에서
  store trait은 concrete repo의 그림자 인터페이스가 되고, 제네릭·mock·async-trait 보일러플레이트만 늘린다.
- DB 제약·trigger·recursive CTE·transaction race는 mock이 정확히 흉내내기 어렵다. 따라서 핵심
  lifecycle/search/permission은 실제 Postgres 통합테스트가 신뢰도 높다.
- 순수 로직을 순수 함수로 분리하면 mock 없이도 빠른 유닛테스트가 된다. 테스트성을 잃는 게 아니라
  올바른 레이어로 옮기는 것이다.
- 테스트 속도 비용은 추상화 부재가 아니라 테스트 셋업(마이그레이션-per-test)에서 온다. 따라서
  추상화가 아니라 인프라로 해결하는 것이 커버리지 손실 없이 맞다.

## 결과

- service store trait과 mock store 테스트는 두지 않는다.
- 순수 함수 테스트(content/patch/policy/validation/cursor/pagination)는 유지·확장한다.
- repo 자체 동작은 db 통합 테스트, files/search/permissions/lifecycle 같은 end-to-end 성격은
  service 통합 테스트로 둔다.
- 테스트 속도 최적화는 테스트 인프라 레벨에서 한다.
- 다형성 seam은 실제 runtime boundary에만 둔다.
