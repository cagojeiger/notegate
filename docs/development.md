# 개발 가이드

## 저장소 구조

```text
notegate/
├─ backend/crates/
│  ├─ api/                     # Axum server, REST/MCP/auth, static web
│  ├─ jobs/                    # PostgreSQL job queue와 worker runtime
│  ├─ media/                   # byte-based media detection와 bounded DOCX validation
│  ├─ search/                  # find/grep 실행, matcher, body cache와 search telemetry
│  ├─ service/                 # business logic과 command semantics
│  ├─ db/                      # sqlx pool, repository, migration
│  ├─ model/                   # shared domain type
│  ├─ core/                    # config, limit, validation, shared error
│  └─ text/                    # pure text metrics, logical lines, edits, syntax checks
├─ frontend/web/               # React dashboard
├─ deploy/
│  ├─ docker/web.Dockerfile
│  ├─ nginx/notegate.conf
│  └─ observability/
└─ docker-compose.yml
```

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
NOTEGATE_AUTHGATE_URL
NOTEGATE_PUBLIC_URL
NOTEGATE_OAUTH_CLIENT_ID
NOTEGATE_MCP_OAUTH_CLIENT_ID
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
| `make test-fast` | DB/S3 환경변수를 해제하고 통합 테스트 제외 안내; 전체 검증으로 취급하지 않음 |
| 직접 `cargo test` | 환경변수 없는 통합 테스트가 조기 종료할 수 있고 run 종료 cleanup은 통합 실행 명령에만 적용 |

`make frontend-check`는 dependency audit, theme contrast, typecheck, lint, unit test와 production build를 실행한다.

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
