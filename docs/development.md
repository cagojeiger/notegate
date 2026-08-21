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

기본 `NOTEGATE_PROCESS_MODE=all`은 HTTP server와 background job runtime을 함께 실행한다.
운영에서는 같은 image를 `api`와 `worker` mode로 나눌 수 있다. Worker mode의 HTTP listener는
`/health`, `/ready`, 활성화된 `/metrics`만 제공한다.
Process mode는 실행할 component만 선택하며, 세 mode 모두 동일한 전체 runtime 설정을 읽고 검증한다.

```sh
cargo run --bin notegate-api
pnpm web:dev
```

| Service | URL |
|---|---|
| Dashboard | `http://localhost:5173` |
| API/MCP | `http://localhost:9191` |
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

`web` image는 dashboard와 Rust server를 포함한다. Proxy는 NoteGate를 `http://localhost:9191`에 노출하고 Compose는 PostgreSQL, MinIO, Prometheus, Grafana와 로컬 bucket 초기화 job을 함께 실행한다. Compose는 `all` mode를 사용하며 `NOTEGATE_BACKGROUND_JOBS__CONCURRENCY`는 각 replica에 전달된다.

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
