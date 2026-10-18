# ADR 0003: 코드 구조와 테스트 전략

## 맥락

notegate 백엔드는 `core · text · media · model · db · service · search · command · api`를 중심으로 구성하고,
`cli`가 HTTP Command API의 외부 adapter 역할을 한다. 구조와 테스트 전략은 PostgreSQL 중심의
단일 제품 백엔드라는 전제에 맞춘다.

두 가지 사실이 구조와 테스트 방향을 결정한다.

- **DB는 PostgreSQL로 고정한다.** Service와 search는 PostgreSQL concrete repo를 사용한다.
- **DB의 정확성은 SQL 제약·트리거·recursive CTE·트랜잭션 경쟁·UNIQUE에 있다.** 이 부분은 mock이
  충실히 흉내낼 수 없고, mock 기반 테스트는 거짓 안심을 준다.

## 결정

### 1. mock을 최소화한다 — store-trait DIP를 두지 않는다

- 단일 production 구현뿐인 store trait(`FilesStore`, `SpaceStore`, `AccountStore` 등)을 두지
  않는다. service와 search는 `notegate-db`의 concrete repo를 직접 소유·사용한다.
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
api transport adapters ──▶ api commands ──┬─▶ service ──▶ db ──▶ model ──▶ core
          │                    │           └─▶ search ───▶ db
          └────────────────────┴─▶ command ◀────────────────────── cli

(api는 조립을 위해 db/model도 직접 참조 가능)
```

- DB는 PostgreSQL로 고정하며 service와 search는 concrete repository를 사용한다.
- 순수 텍스트·미디어 처리는 transport와 DB에서 분리한다. 권한·quota·transaction orchestration은 service가 소유한다.
- Command schema·validation·recovery는 command가 소유하고 CLI는 API 구현에 의존하지 않는다.
- MCP와 HTTP adapter는 인증·protocol/error mapping만 담당하고 같은 command 실행기를 호출한다.
- Search 실행·cache·admission은 private listener가 소유하며 public API에는 client만 둔다.
- 파일 변경 문맥의 출처·편집 세션·호출 ID는 인증된 actor·권한과 별개다. 새 요청에 이전 문맥을 이어받지 않는다.

### 4. 구조 컨벤션 (보조)

- service 도메인은 관심사 기준으로만 분리한다. 단순히 메서드 수가 많다는 이유로 trait/mock 구조를
  만들지 않는다.
- db repo 파일명은 구조체 `XxxRepo`에 대응해 `<domain>_repo.rs`로 통일한다.
- API 표면은 표면별 사용성에 맞춰 조직한다(REST는 리소스별, MCP는 identity와
  read/search/write/manage 행동 영역과 ordered command sequence별). 공통 command DTO/schema는 command가,
  표면 고유 DTO/schema는 해당 api adapter가 책임진다.
- 공통 헬퍼는 실제 중복이 있을 때만 모은다(표면 내부 공유는 `support.rs`, 크레이트 전역 공유는
  크레이트 루트).

## 근거

단일 구현을 감싼 store trait은 교체 가능성을 제공하지 않으면서 interface와 mock을 중복시킨다.
DB 제약·trigger·recursive CTE·transaction race는 실제 PostgreSQL에서 검증한다. 테스트 속도는
업무 추상화를 추가하기보다 schema 격리와 CI 인프라로 관리한다.
