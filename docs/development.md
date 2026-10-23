# 개발 가이드

## 로컬 개발

Dashboard와 API를 분리해 실행한다.

```sh
pnpm install
cp .env.example .env
make dev-infra
```

기본 `NOTEGATE_PROCESS_MODE=all`이며 같은 image를 다음 process role로 실행한다. 모든 role은 전체 runtime 설정을 읽고 검증한다.

| Mode | 실행·DB 초기화 |
|---|---|
| `all` | Public HTTP + worker + reconciler + local/remote Search; migration/usage bootstrap |
| `api` | Public HTTP + local/remote Search; migration/usage bootstrap |
| `worker` / `reconciler` | 해당 background runtime; schema/active crypto epoch read-only 검증 |
| `search` | Private Search; schema/active crypto epoch read-only 검증 |

Local Search가 있으면 Public/Search는 같은 process에서도 기본 `9191`/`9192`의 별도 socket이다. Worker/reconciler listener와 독립 `search` mode의 private listener는 `/health`, `/ready`, 활성화된 `/metrics`를 control plane으로 제공한다. Local Search를 포함한 `all`/`api`의 `/metrics`는 public listener에만 등록한다.

```text
NOTEGATE_SEARCH_BIND_ADDR=127.0.0.1:9192  # default, all/api local search
# Leave NOTEGATE_SEARCH_SERVICE_URL unset to use the local listener.
# NOTEGATE_SEARCH_SERVICE_URL=http://notegate-search:9192
# Optional: local/standalone search reads through a distinct Postgres pool.
# NOTEGATE_READ_DATABASE_URL=postgres://notegate:notegate@read-replica:5432/notegate
# NOTEGATE_READ_DB_MAX_CONNECTIONS=10
```

특정 Helm/배포 도구에 의존하지 않는 runtime topology:

| Topology | Main process | Search URL | Additional processes |
|---|---|---|---|
| combined | `all` | unset | none |
| search split | `all` | internal Search URL | `search` |
| background split | `api` | unset | `worker`, `reconciler` |
| full split | `api` | internal Search URL | `search`, `worker`, `reconciler` |

각 process는 자신의 control-plane metric endpoint만 소유한다. Search URL은 Search 실행 위치만
바꾸며 `all` mode의 worker와 reconciler를 끄지 않는다.

검색 전용 pod는 `NOTEGATE_PROCESS_MODE=search`와 `NOTEGATE_SEARCH_BIND_ADDR=0.0.0.0:9192`를
사용한다. API pod는 `NOTEGATE_PROCESS_MODE=api`와 내부 Service의 root URL인
`NOTEGATE_SEARCH_SERVICE_URL=http://notegate-search:9192`를 사용한다. 이 URL에는 path, query,
credential을 넣지 않는다. 서명·private route·DB 읽기 경계는 [Search](spec/search.md#execution-boundary)가 정본이다. `NOTEGATE_READ_DATABASE_URL`이 없으면 primary handle을 공유한다. 별도 read endpoint는 권한 철회의 즉시성을 유지하지만 본문 검색에 replica lag가 보일 수 있다. 쓰기·worker·reconciler는 primary를 사용하며 remote Search를 호출하는 API/background role은 read pool을 만들지 않는다.

```sh
cargo run --bin notegate-api
pnpm web:dev
```

| Service | URL |
|---|---|
| Dashboard | `http://localhost:5173` |
| API/MCP | `http://localhost:9191` |
| Search internal | `http://127.0.0.1:9192` |
| PostgreSQL | `localhost:5433` |
| MinIO S3 API | `http://localhost:9000` |
| MinIO console | `http://localhost:9001` |

```sh
curl localhost:9191/health
curl localhost:9191/ready
```

## Docker Compose

```sh
cp .env.example .env
make up
```

`web` image는 dashboard와 Rust server를 포함한다. Proxy는 public listener만 `http://localhost:9191`에 노출하고 Compose는 PostgreSQL, MinIO, Prometheus, Grafana와 로컬 bucket 초기화 job을 함께 실행한다. Compose는 `all` mode를 사용하고 private search listener는 container loopback에 유지한다. `NOTEGATE_BACKGROUND_JOBS__CONCURRENCY`는 각 replica에 전달된다.

기존 PostgreSQL 17 개발 데이터를 18로 옮길 때는 [major upgrade 절차](https://www.postgresql.org/docs/18/upgrading.html)를 따른다. 기존 `pgdata` 이름을 유지하므로 17 데이터가 있으면 공식 18 entrypoint가 초기화를 거부한다.

1. NoteGate API·worker 등 모든 쓰기를 중단하고, 17 image와 기존 `/var/lib/postgresql/data` mount로 DB를 열어 `pg_dump --format=custom` 백업을 만든다.
2. 별도 새 18 볼륨에 백업을 복원하고 데이터·역할·확장을 확인한다. 기존 17 볼륨과 백업은 보관한다.
3. 개인 Compose override의 `volumes.pgdata.name`을 복원한 18 볼륨으로 지정한다. 기본·split stack 모두 `docker compose -f <base-file> -f <override-file>`처럼 같은 override를 명시해 실행한다.
4. API 준비 상태와 기존 데이터가 확인되면 쓰기를 재개한다. 18에서 쓰기가 발생한 뒤에는 이전 image만 지정해 복구할 수 없으므로 백업 복원과 후속 쓰기 처리를 함께 판단한다.

최종 runtime은 digest로 고정한 `gcr.io/distroless/cc-debian13:nonroot`이며 UID/GID
`10001:10001`로 실행한다. Rust/Node 빌드 도구는 build stage에만 있고, runtime의 glibc,
libgcc와 CA 인증서는 Distroless가 제공한다. Runtime에는 shell이나 package manager가
없으므로 `docker exec ... sh`를 사용할 수 없다. 상태 확인은 `/health`, `/ready`와
외부 probe container를 사용하고, 진단 도구는 별도 debug container에서 실행한다.

완전 분리 실행 계약은 `docker-compose.split.yml`로 검증한다. 이 stack은 `api`,
`search`, `worker`, `reconciler`를 각각 다른 process로 실행하고 Prometheus가 네
control plane의 `/metrics`를 독립적으로 scrape한다. 실행과 검증은
`make split-up`, `make split-test`를 사용하며 상세 계약은
[`deploy/observability/README.md`](../deploy/observability/README.md)를 따른다.

| Service | URL |
|---|---|
| NoteGate | `http://localhost:9191` |
| Prometheus | `http://localhost:9090` |
| Grafana | `http://localhost:3000` |

Grafana의 기본 로컬 계정은 `admin` / `notegate-local`이다. Dashboard 구성, 검증과 Kubernetes packaging은 [`deploy/observability/README.md`](../deploy/observability/README.md)를 따른다.

Application metric은 기본적으로 비활성화되어 있다. Compose는 기본값이 `true`인 `COMPOSE_NOTEGATE_METRICS_ENABLED`를 `NOTEGATE_METRICS_ENABLED`로 전달하고, Prometheus는 각 `web` replica의 `/metrics`를 수집한다.

```sh
make curl-metrics
```

## Object storage

S3 설정은 API 시작에 필수다. Bucket은 운영자가 미리 생성하며 NoteGate는 설정된 기존 bucket만 사용한다.

필수 runtime 설정:

```text
NOTEGATE_S3__ENDPOINT
NOTEGATE_S3__REGION
NOTEGATE_S3__BUCKET
NOTEGATE_S3__ACCESS_KEY
NOTEGATE_S3__SECRET_KEY
```

브라우저가 내부 endpoint에 접근할 수 없으면 `NOTEGATE_S3__PUBLIC_ENDPOINT`도 설정한다. `NOTEGATE_S3__FORCE_PATH_STYLE`은 기본 `true`이며 provider에 맞게 변경한다. Access key와 secret key는 secret manager에서 주입한다.

브라우저가 `PUBLIC_ENDPOINT`로 직접 PUT/GET할 수 있도록 provider CORS는 다음을 허용해야 한다.

- Origin: NoteGate origin
- Method: `PUT`, `GET`
- Request header: `Content-Type`, `If-None-Match`
- Exposed response header: `ETag`

Multipart 완료에는 각 part의 `ETag`가 필요하다. `ENDPOINT`는 서버 내부 주소이고 `PUBLIC_ENDPOINT`는 브라우저가 접근하고 서명에 사용하는 주소다.

로컬 MinIO Compose는 버킷별 CORS 대신 `MINIO_API_CORS_ALLOW_ORIGIN`으로 서버 전역 origin을 설정한다. MinIO root account는 초기화에만 사용하고, NoteGate runtime account에는 설정된 bucket의 `objects/*` 아래에서 `GetObject`, `PutObject`, `DeleteObject`, `AbortMultipartUpload`만 허용한다.

## 인증과 MCP

`.env`에서 다음 값을 설정한다.

```text
NOTEGATE_PUBLIC_URL
NOTEGATE_OIDC__ISSUER
NOTEGATE_OIDC__CLIENT_ID
NOTEGATE_CLI_OAUTH_CLIENT_ID
NOTEGATE_ENC_ROOT_KEY_ID
NOTEGATE_ENC_ROOT_SECRET
NOTEGATE_LOOKUP_ROOT_KEY_ID
NOTEGATE_LOOKUP_ROOT_SECRET
```

- OAuth redirect: `${NOTEGATE_PUBLIC_URL}/auth/callback`
- User MCP: `${NOTEGATE_PUBLIC_URL}/mcp`
- Agent MCP: `${NOTEGATE_PUBLIC_URL}/mcp/v2`

Encryption과 lookup root secret은 각각 32 bytes 이상이어야 한다. API는 시작할 때 active key epoch를 검증하며, 환경 변수와 DB registry가 다르면 시작하지 않는다.

상세한 인증·키 계약은 [`docs/spec/security.md`](./spec/security.md), MCP 연결 계약은 [`docs/spec/mcp`](./spec/mcp/README.md)를 따른다.

User MCP 연결은 `${NOTEGATE_PUBLIC_URL}/auth/login`에서 Google 로그인을 마친 뒤 `/mcp`로 연결한다. Agent MCP는 `ngk_v2_` Agent key만 허용하며 OAuth bearer token을 받지 않는다.

## 검증

화면 변경은 desktop·tablet·mobile의 light/dark, 320 CSS px reflow와 키보드 조작을 확인한다. 순수 로직은 단위 테스트, 실제 layout·상호작용·세션은 Playwright로 검증한다. Screenshot만으로 동작을 판정하지 않는다.

```sh
make fmt
make check
make clippy
make test
make frontend-check
git diff --check
```

`make test`(또는 `make test-integration`)는 전체 Rust 검증이다. PostgreSQL 클라이언트
`psql`과 실행 중인 테스트 PostgreSQL/S3가 필요하며, `NOTEGATE_TEST_DATABASE_URL` 또는
`NOTEGATE_TEST_S3_ENDPOINT`가 없거나 비어 있으면 테스트를 시작하지 않고 실패한다.
아래 버킷과 인증 정보는 테스트 S3 설정에 맞추는 예시다.

```sh
export NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5432/notegate
export NOTEGATE_TEST_S3_ENDPOINT=http://127.0.0.1:9000
export NOTEGATE_TEST_S3_BUCKET=notegate-test
export NOTEGATE_TEST_S3_ACCESS_KEY=notegate-app
export NOTEGATE_TEST_S3_SECRET_KEY=notegate-app-secret
make test-integration
# 특정 Rust 테스트만 실행할 때도 실행 후 정리를 적용한다.
deploy/ci/test-rust.sh -p notegate-api rest::file_upload_tests
```

| 검증 범위 | 조건·정리 |
|---|---|
| 통합 실행 | 고유 `NOTEGATE_TEST_RUN_ID`, 테스트별 독립 schema. 개별 cleanup 후 cargo 종료 시 해당 run 잔여 schema만 정리; 다른 실행/기존 schema 유지 |
| 실패 정리 | Assertion/반환 오류/migration 오류도 대상. Cleanup 실패는 명령 실패. SIGKILL/DB 장애는 보장 불가; S3는 개별 테스트가 정리 |
| `make test-fast` | 로컬에서만 `NOTEGATE_TEST_SKIP_INTEGRATION=1`로 DB/S3 테스트를 명시적으로 제외; 전체 검증으로 취급하지 않음. CI에서는 이 우회를 거부 |
| 직접 `cargo test` | 필요한 DB/S3 환경변수가 없거나 비어 있으면 해당 테스트 실패. Run 종료 cleanup은 통합 실행 명령에만 적용 |

Rust 테스트는 외부 서비스 필요 여부로 분리해서 실행할 수 있다.

```sh
make test-unit      # DB/S3 없이 자체 실행 가능한 테스트와 doctest
make test-external  # PostgreSQL/S3가 필요한 테스트만 실행하고 run 잔여 schema 정리
make test-measure   # 동일 바이너리로 전체 harness 실행과 분리 실행을 3회 비교
```

`test-unit`에는 순수 로직 외에 자체 HTTP mock, CLI subprocess, 로컬 fixture 테스트도
포함된다. `src/`·`tests/` 위치 대신 `deploy/ci/rust-test-resources.json`의 첫 일치 규칙으로
분류하며, DB/S3 환경변수와 integration skip 우회를 제거한다. 새 external 테스트는
분류 규칙도 갱신한다. 분류되지 않은 target, 그룹 간 동일 이름 충돌, 실제 실행된
테스트 ID의 누락·중복은 실패다. 기존 ignored 테스트는 계속 ignored이며 CI의 명시적
성능 측정은 별도로 유지한다. `make test`와 직접 Cargo 전체 실행의 범위는 유지된다.

컴파일·그룹별 실행·doctest 시간과 정확한 테스트 목록은 `target/test-suites/`에 기록된다.
`test-measure`는 첫 컴파일 후 baseline→분리, 분리→baseline 순서를 번갈아 실행한다.
Baseline은 일반 테스트 바이너리 전체이며 doctest는 모든 반복 뒤 한 번 별도 실행한다.
CI에서는 `Rust Build`가 정적 검사, 테스트 컴파일·inventory 생성, doctest와 API·seed
빌드를 담당한다. 테스트 실행 파일과 CLI subprocess용 실행 파일은 `rust-prepared-tests`,
API·seed는 `rust-e2e-api` artifact로 전달한다. `Rust`는 두 테스트 그룹을, `Browser E2E`는
API·Web 기능을 실행하며 두 job에서 Cargo 재빌드는 하지 않는다. Browser는 `Rust Build`,
Web, Hygiene 완료 후 Rust 테스트와 병렬로 진행한다. 빌드 실패는 필수 `Rust` 검사도 실패시킨다.
전달한 파일은 같은 checkout 경로·커밋·실행·플랫폼·toolchain에서만 사용하고, package별
작업 디렉터리, 테스트 분류·누락 검증, run 단위 DB 정리를 유지한다. 기존 로컬 Make 명령은
직접 컴파일·실행하는 경로를 유지한다. 빌드 시간과 artifact 전송 시간은 테스트 실행 시간과
별도로 비교한다. PostgreSQL 18과 배포용 release 빌드는 각자의 빌드 경로를 유지한다.
DB helper는 `NOTEGATE_TEST_TIMING_FILE`의 절대 경로에 opt-in JSONL을 기록한다.
Setup·migration·초기 fixture·cleanup 시간과 setup 완료부터 cleanup 시작까지의 lifetime을
구분한다. Lifetime은 순수 테스트 본문 시간과 다르며, 병렬 테스트의 시간 합계도 전체
wall time과 다르다. Setup 실패와 개별 cleanup 미호출은 별도로 해석해야 한다.

DB 없는 단위 검증에는 실행·저장 의존성을 명시적으로 주입한다. Invocation history는
작은 writer 경계, 작업 handler는 lease monitor future, 검색 runtime은 실행 closure로
검증할 수 있다. 기록 대역도 payload·실패·완료 순서를 관찰하며 실제 SQL 저장과
트랜잭션 감사 검증은 통합 테스트에 남긴다. 사용하지 않는 저장소를 만들기 위해
접속 불가능한 주소나 닫힌 pool을 새로 넣지 않는다.

시간 경계는 가능한 한 고정된 `now` 입력으로 검사하고, 작업 순서·경합은 완료 신호나
실제 lock 결과를 관측한다. PostgreSQL 시간이 기준인 검증은 DB에서 읽은 시간으로
비교한다. 고정 sleep으로 다른 task의 진행을 가정하지 않으며 timeout 안의 작업 오류도
반드시 확인한다. 실제 lease 갱신·네트워크 deadline·성능 기준을 검증하는 통합 테스트의
시간 조건은 해당 계약과 함께 유지한다.

브라우저 CI는 `.only`와 재시도로 통과한 flaky test도 실패로 처리한다. 재시도는 진단을
위해 한 번 유지한다. 실제 dashboard smoke는 테스트 세션이 없으면 건너뛰지 않고 실패한다.

`make frontend-check`는 dependency audit, theme contrast, typecheck, lint, unit test와 production build를 실행하고, 생성한 번들의 크기 예산을 검사한다.

### CI 실행 경계

| 시점 | 실행 범위 |
| --- | --- |
| PR·main CI | Rust 단위·외부 의존성 테스트, Web 검사·번들 크기 예산, browser 기능·접근성 검증, 실제 API의 Workbench·OAuth 계약 |
| PR | 의존성 변경 검사, RustSec, production 이미지 빌드·취약점 검사, 관련 변경의 DAST 파일럿 |
| 실제 릴리즈 게시 성공 후·수동 | Workbench 성능 3회 반복, Lighthouse 3회, 텍스트 쓰기 기준 버전 비교 |
| 정기 보안 검사 | 최신 advisory에 대한 의존성·발행 이미지 검사 |

Web job이 manifest를 포함한 production `dist`를 한 번 만들고, 같은 CI 실행의 Browser E2E가
`web-dist` artifact를 받아 API의 정적 파일 서빙으로 live Workbench를 검증한다. 기존 mock 기반
E2E는 Vite 개발 서버를 사용한다. 릴리즈 측정은 성공한 main CI의 run ID와 commit SHA가
일치하는 artifact를 사용하며, 수동 측정은 선택한 commit에서 빌드한다.

`playwright.functional.config.ts`의 Workbench는 한 번 실행하고, standalone OAuth 4개 검증은
별도 identity로 실행한다. 각 project 직전에 identity를 생성해 이력·휴지통 fixture를 격리하고,
5분짜리 일회용 OAuth 동의를 재사용하지 않는다. 이 검증은 trace와 retry를 끈다.

`Browser Performance`와 `Text Write Performance`는 PR에서 실행하지 않는다. Release의
`should_release` 판정과 실제 게시 성공을 모두 확인한 뒤, 게시한 commit SHA로 호출한다.
텍스트 쓰기는 직전 실제 공개 릴리즈의 고정 SHA와 비교하며, 기준 버전이 없으면 비교 불가를
명시한다. 수동 기준도 후보 커밋의 조상만 지정할 수 있다. 측정 실패는 이미 게시한 릴리즈를
되돌리지 않는다.

빌드한 번들만 검사하려면 `pnpm --filter web check:preview-bundle`, 같은 번들로 Lighthouse를
실행하려면 `pnpm --filter web test:lighthouse:built`를 사용한다. 기존 `test:lighthouse` 명령은
로컬 편의를 위해 빌드부터 실행한다.

### 동적 보안 테스트 파일럿

GitHub Actions의 `Security DAST Pilot`은 관련 렌더링 코드·테스트 파일의 PR과 수동 실행에서 동작한다.
격리된 CI DB·스토리지, 두 소유자와 임시 Agent로 실제 HTTP 권한 검증과 악성 문서의 Chromium 렌더링을 검사한다.
고정된 CI 대상만 허용하며 운영 데이터·인증 서버·credential을 사용하지 않는다.
ZAP은 별도 internal Docker network에서 합성 Space의 명시적 요청만 검사한다.
Active 규칙은 path traversal·reflected XSS·SQL injection이며 단일 thread, 500ms 간격, 5분 제한을 둔다.
Scanner의 인터넷 접근은 차단하지만 internal bridge gateway의 host 서비스 접근까지 차단하는 네트워크 정책은 아니다.
권한·렌더링 불변식 위반과 설정·인증·검사 미실행은 실패로 남긴다. ZAP alert는 자동 차단 없이 재현·분류한다.
정제된 JSON·HTML·요약·실패 스크린샷을 `security-dast-pilot` artifact에 14일 보관하며 credential plan과 원본 ZAP 보고서는 업로드하지 않는다.
이 파일럿은 운영 ingress/WAF, 전체 OAuth 로그인, 부하 한계 또는 전체 침투 테스트를 검증하지 않는다.

```sh
pnpm --filter web exec playwright install chromium
pnpm --filter web test:e2e
pnpm --filter web test:lighthouse
```

Playwright는 login과 주요 authenticated workspace flow를 desktop, tablet과 mobile viewport에서 검증한다. Axe 기반 WCAG 2.2 AA 검사는 login, Space Library와 file preview 등 적용된 spec에서 실행한다. Lighthouse 결과는 lab regression 신호이며 production Core Web Vitals는 별도 field monitoring이 필요하다.

### 텍스트 엔진 독립 테스트

텍스트 엔진은 PostgreSQL이나 API 없이 검증할 수 있다.

```sh
cargo test -p notegate-text
```

### 미디어 판별 독립 테스트

파일 형식 판별과 DOCX의 ZIP/XML 검증은 PostgreSQL, S3, API 없이 실행한다.
압축 크기·해제 크기·경로·XML 제한은 media에 유지하고, 미리보기 허용 정책과
저장소 읽기·동시 실행 제한은 API 테스트에서 검증한다.

```sh
cargo test -p notegate-media
```

## 보안과 의존성 관리

- Swagger UI는 정확히 고정한 `utoipa-swagger-ui-vendored`의 공식 자산을 사용한다.
  API의 `build.rs`가 workspace `zip`으로 필요한 고정 파일만 읽어 바이너리에 내장한다.
  자산 버전을 변경할 때는 빌드의 archive 경로와 문서 인증·실제 브라우저 검사를 함께 확인한다.
- PR의 `Hygiene`는 `make version-check`로 `VERSION`, workspace package version,
  각 crate의 `version.workspace`, `Cargo.lock`의 로컬 package version을 비교한다.
  이 검사는 Python 3.11 이상이 필요하며 의존성을 다운로드하지 않는다.
  `frontend/web/package.json`의 private package version은 제품 릴리즈 버전이 아니다.
  Dashboard는 기존대로 루트 `VERSION`을 읽는다.
- `package.json`의 `packageManager`가 pnpm 버전의 기준이며 CI와 Docker가 이를 사용한다.
  CI Node 버전은 `.node-version`에서 관리하고 Docker의 정확한 Node tag와 비교한다.
  Rust toolchain channel도 Docker Rust tag와 일치해야 `version-check`를 통과한다.
  Node major 업데이트는 LTS 지원 여부를 확인한 뒤 CI와 Docker를 함께 변경한다. Rust compiler 변경 시 `rust-toolchain.toml`, workspace의 `rust-version`, Docker Rust
  base image를 함께 검토한다. Docker base image는 digest로 고정하고 cargo-chef와 sccache는
  `--version`과 `--locked`로 고정한다. 두 빌드 도구의 버전은 수동 검토 대상이다.
- Dependabot은 Cargo, npm, Actions, Docker/Compose를 매주 월요일 09:00 KST에 검사한다.
  minor/patch 업데이트는 묶고 major 업데이트는 별도 PR로 검증한다. Digest 갱신도 PR로
  검토한다. 자동 업데이트 PR은 자동 머지를 의미하지 않는다.
- `Dependency Security`는 PR에서 dependency review와 RustSec 감사를 수행한다.
  Rust 감사는 yanked crate도 실패시킨다. 매주 또는 수동 실행 시 Rust와 npm 모두 감사한다.
  PR/main의 `Web` 검사와 `make frontend-check`도 개발 의존성을 포함해 npm 감사를 수행한다.
- PR의 `PR Image Gate`는 자격 증명 없이 linux/amd64 이미지를 로컬 빌드한다.
  HIGH/CRITICAL은 수정 가능 여부와 관계없이 보고하고, 수정 가능한 CRITICAL만 실패시킨다.
  릴리즈에서는 최종 linux/amd64·linux/arm64 이미지를 각각 digest로 푸시한 뒤 같은
  기준으로 검사한다. 두 검사가 통과해야 버전/`latest` 태그와 GitHub Release가 만들어진다.
  릴리즈에는 index·아키텍처별 digest를 담은 `image-digests.json`과 검사 JSON을 보존한다.
  스캔 전에 푸시한 태그 없는 digest는 레지스트리에 남을 수 있다.
- `Image Security`는 매일 09:20 KST 또는 수동 실행 시 GHCR의 `latest` 이미지를 Trivy로 검사한다.
  실행 시작 시 digest를 고정하고 linux/amd64와 linux/arm64를 각각 검사한다.
  수정 버전이 없는 HIGH/CRITICAL도 보고하며 수정 가능한 CRITICAL이 있으면 실행을 실패시킨다.
  JSON/SARIF/텍스트 보고서는 Actions artifact에 30일 보존하고, 일일·수동 실행 결과는
  GitHub Security의 code scanning에 아키텍처별로 게시한다. 워크플로 변경 PR도 현재 발행
  이미지를 검사하되 Security 결과를 게시하지 않는다. 이 검사는 이미지에서 식별 가능한
  패키지를 대상으로 하며, Rust 바이너리와 프론트엔드 번들의 소스 의존성 감사는 위 검사를 유지한다.
  `latest` 재검사는 운영 배포 digest의 증거가 아니다. GitOps는 선언된 운영 이미지를 별도로
  재검사하고, 실제 배포 확인에는 Argo 상태와 Pod imageID를 확인한다.
- `.cargo/audit.toml`의 `RUSTSEC-2023-0071` 예외는 openidconnect의 RSA 서명 검증 경로에
  한정한다. 네트워크에서 관찰 가능한 RSA 개인키 연산을 추가하거나 upstream 수정 버전이
  나오면 즉시 재검토한다.

GitHub 설정은 소스와 별도로 관리한다. 아래는 운영 설정 확인 목록이며 현재 UI 상태의 증거가 아니다.

| 설정 | 확인할 항목 |
|---|---|
| Main ruleset | 최신 base, GitHub Actions 제공자의 `Hygiene`, `Rust`, `Web`, `Browser E2E`, `Dependency Review`, `RustSec Audit`, `PR Image Gate` required checks |
| CodeQL | 필수 분석, 새 medium 이상 보안 경고/error 분석 경고 차단 |
| Actions | Full commit SHA, 기본 token read-only, PR 승인 권한 없음 |
| Security | Secret scanning, push protection, Dependabot security updates, 비공개 취약점 제보 |

CodeQL 기본 설정은 Actions와 JS/TS의 extended query suite와 remote/local threat model을
사용한다. Rust는 현재 기본 설정 API가 허용하지 않아 이 CodeQL 범위에 포함되지 않는다.
Rust의 compiler, clippy, 테스트, RustSec 검사는 별도로 유지한다. 저장소 설정이나 감사
통과는 애플리케이션 전체가 안전하다는 보증을 의미하지 않는다.
