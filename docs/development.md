# Development guide

## Repository layout

```text
notegate/
├─ backend/crates/
│  ├─ api/                     # Axum server, REST/MCP/auth/static web serving
│  ├─ service/                 # Business logic and command semantics
│  ├─ db/                      # sqlx pool, repositories, and migrations
│  ├─ model/                   # Shared domain types
│  └─ core/                    # Config, limits, validation, and shared errors
├─ frontend/web/               # React dashboard
├─ deploy/
│  ├─ docker/web.Dockerfile    # Frontend build and backend binary
│  ├─ nginx/notegate.conf      # Reverse proxy for scaled web replicas
│  └─ observability/           # Prometheus and provisioned Grafana dashboards
└─ docker-compose.yml          # App, storage, proxy, and observability stack
```

## Local development

Development keeps the dashboard and API separate for fast feedback.

```sh
# Install frontend dependencies.
pnpm install

# Configure local settings.
cp .env.example .env

# Start PostgreSQL and MinIO.
make dev-infra
```

Run the API and dashboard in separate terminals:

```sh
cargo run --bin notegate-api
pnpm web:dev
```

Default local services:

- Dashboard: `http://localhost:5173`
- API and MCP: `http://localhost:9191`
- PostgreSQL: `localhost:5433`
- MinIO S3 API: `http://localhost:9000`
- MinIO console: `http://localhost:9001`

Health checks:

```sh
curl localhost:9191/health
curl localhost:9191/ready
```

## Production-like Docker stack

The `web` image contains the built dashboard and Rust server. The server handles `/api`, `/auth`, and `/mcp`, and serves the browser app.

```sh
cp .env.example .env
make up
```

The stack exposes NoteGate through the proxy at `http://localhost:9191`. It also
starts PostgreSQL, MinIO, Prometheus, Grafana, and an initialization job that creates
the local bucket and its least-privilege application account.

Local observability endpoints:

- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3000`
- Grafana login: `admin` / `notegate-local` by default
- Provisioned dashboards: **NoteGate / Service Overview** and
  **NoteGate / Search Detail**

The application-level metrics default remains disabled. Docker Compose intentionally
sets `NOTEGATE_METRICS_ENABLED` from `COMPOSE_NOTEGATE_METRICS_ENABLED`, which defaults
to `true` for this local stack. Prometheus discovers both scaled `web` replicas through
Docker DNS and scrapes each process-local `/metrics` endpoint. Change the local ports
or Grafana credentials in `.env`.

Verify the three endpoints after `make up`:

```sh
make curl-metrics
```

The MinIO root account is used only for local initialization. The NoteGate runtime account is limited to `GetObject`, `PutObject`, and `DeleteObject` under the configured bucket's `objects/*` prefix. It cannot create or list buckets.

## Authentication and MCP

Authentication and MCP settings come from `.env`:

- `NOTEGATE_AUTHGATE_URL`
- `NOTEGATE_PUBLIC_URL`
- `NOTEGATE_OAUTH_CLIENT_ID`
- `NOTEGATE_MCP_OAUTH_CLIENT_ID`
- `NOTEGATE_ENC_ROOT_KEY_ID` and `NOTEGATE_ENC_ROOT_SECRET`
- `NOTEGATE_LOOKUP_ROOT_KEY_ID` and `NOTEGATE_LOOKUP_ROOT_SECRET`

The OAuth redirect URL defaults to `${NOTEGATE_PUBLIC_URL}/auth/callback`. The MCP resource URL defaults to `${NOTEGATE_PUBLIC_URL}/mcp`.

The encryption and lookup root secrets must each be at least 32 bytes. On startup, the API ensures and verifies the configured active key epochs. Startup fails if the database already has a different active key for either domain; key rotation is not automatic.

For a first MCP connection, open `${NOTEGATE_PUBLIC_URL}/auth/login`, complete Google sign-in, and reconnect the client to `${NOTEGATE_PUBLIC_URL}/mcp`.

## Checks

```sh
make fmt
make check
make clippy
make test
make frontend-check
git diff --check
```

`make frontend-check` runs the production dependency audit, theme contrast check,
typecheck, lint, unit tests, and production build. Browser quality checks use:

```sh
pnpm --filter web exec playwright install chromium
pnpm --filter web test:e2e
pnpm --filter web test:lighthouse
```

The Playwright suite runs WCAG 2.2 AA axe checks for the login and authenticated
workspace across desktop, tablet, and mobile layouts. Lighthouse CI checks the
production build with performance, accessibility, and best-practice budgets.
Its LCP, CLS, and TBT results are lab regression signals; production INP and
other Core Web Vitals still require field monitoring after deployment.
