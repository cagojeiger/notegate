# 개발 가이드

## 저장소 구조

```text
notegate/
├─ backend/crates/
│  ├─ api/                     # Axum server, REST/MCP/auth, static web
│  ├─ jobs/                    # PostgreSQL job queue와 worker runtime
│  ├─ search/                  # find/grep 실행, matcher, body cache와 search telemetry
│  ├─ service/                 # business logic과 command semantics
│  ├─ db/                      # sqlx pool, repository, migration
│  ├─ model/                   # shared domain type
│  └─ core/                    # config, limit, validation, shared error
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

기본 `NOTEGATE_PROCESS_MODE=all`은 public HTTP, background job worker, reconciliation runtime과 private search HTTP를
함께 실행한다. Public listener와 search listener는 같은 process에서도 각각 `9191`, `9192`로
분리된다. 운영에서는 같은 image를 `api`, `worker`, `reconciler`, `search` mode로 나눌 수 있다.
Worker와 reconciler mode의 HTTP listener 및 search mode의 private listener는 `/health`, `/ready`,
활성화된 `/metrics`만 control plane으로 제공한다. Process mode는 실행할 component만 선택하며, 모든 mode가 동일한
전체 runtime 설정을 읽고 검증한다. Database migration과 usage bootstrap은 `all`/`api` mode가
소유한다. 독립 `worker`/`reconciler`/`search` process는 schema readiness와 active crypto key
epoch를 read-only로 검증한다.

```text
NOTEGATE_SEARCH_BIND_ADDR=127.0.0.1:9192  # default, all/api local search
# Leave NOTEGATE_SEARCH_SERVICE_URL unset to use the local listener.
# NOTEGATE_SEARCH_SERVICE_URL=http://notegate-search:9192
# Optional: local/standalone search reads through a distinct Postgres pool.
# NOTEGATE_READ_DATABASE_URL=postgres://notegate:notegate@read-replica:5432/notegate
# NOTEGATE_READ_DB_MAX_CONNECTIONS=10
```

동일 binary는 다음 네 topology를 지원한다. 이 표는 runtime 조립 계약이며 Helm이나 특정 배포
도구를 전제로 하지 않는다.

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
credential을 넣지 않는다. Private request와 response는 LOOKUP root에서 분리 파생된 HMAC key로
서명되며 public listener에는 `/internal/*` route가 등록되지 않는다.

`NOTEGATE_READ_DATABASE_URL`이 없으면 search는 primary pool handle을 공유한다. 값이 있으면 search
scope, candidate, body와 result hydration은 별도 read pool을 사용하지만 권한 판정은 항상 primary에서
수행한다. 쓰기, queue worker와 reconciliation도 primary pool을 사용한다. 별도 read endpoint를 선택하면
권한 철회의 즉시성은 유지되지만 변경 직후 검색 결과 자체에는 replica lag가 보일 수 있다. Read pool은
로컬 search listener를 소유한 process에서만 생성되며, remote search를 호출하는 API와 background role은
불필요한 read connection을 만들지 않는다.

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

`make frontend-check`는 dependency audit, theme contrast, typecheck, lint, unit test와 production build를 실행한다.

```sh
pnpm --filter web exec playwright install chromium
pnpm --filter web test:e2e
pnpm --filter web test:lighthouse
```

Playwright는 login과 주요 authenticated workspace flow를 desktop, tablet과 mobile viewport에서 검증한다. Axe 기반 WCAG 2.2 AA 검사는 login, Space Library와 file preview 등 적용된 spec에서 실행한다. Lighthouse 결과는 lab regression 신호이며 production Core Web Vitals는 별도 field monitoring이 필요하다.
