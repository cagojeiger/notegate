# notegate

A personal file-tree notes service with a Rust backend (Axum + Tokio + sqlx + PostgreSQL)
and a React dashboard. Browser login and MCP/API access are authenticated through authgate.

## Layout

```
notegate/
├─ Cargo.toml                  # Rust workspace root
├─ package.json                # frontend workspace scripts
├─ pnpm-workspace.yaml
├─ backend/crates/
│  ├─ api/                     # Axum server, REST/MCP/auth/static web serving
│  ├─ service/                 # business logic and command semantics
│  ├─ db/                      # sqlx pool, repositories, migrations
│  ├─ model/                   # shared domain data types
│  └─ core/                    # config, limits, validation, shared error type
├─ frontend/web/               # React dashboard
├─ deploy/
│  ├─ docker/web.Dockerfile    # production-like image: FE build + BE binary
│  └─ nginx/notegate.conf      # local reverse proxy for scaled web replicas
└─ docker-compose.yml          # postgres + scaled web + proxy
```

## Local development

Development keeps the dashboard and API separate for fast feedback.

```sh
# 1. Start Postgres
make dev-db

# 2. Configure env
cp .env.example .env

# 3. Run the API (migrations run on startup)
cargo run --bin notegate-api

# 4. Run the dashboard
pnpm web:dev
```

Default local URLs:

- Dashboard dev server: `http://localhost:5173`
- API/MCP server: `http://localhost:9191`
- Postgres: `localhost:5433`

Health checks:

```sh
curl localhost:9191/health
curl localhost:9191/ready
```

## Local production-like stack

This builds one `web` image containing both the Rust server and the built dashboard.
The Rust server handles `/api`, `/auth`, `/mcp`, and serves the SPA for browser routes.

```sh
cp .env.example .env
make up
```

Equivalent command:

```sh
docker compose up -d --build --remove-orphans
```

Services:

- `proxy` -> public `http://localhost:9191`
- `web` -> two FE+BE replicas inside the compose network
- `postgres` -> local database on host port `5433`

The web image sets `NOTEGATE_WEB_DIST_DIR=/app/web`, so the Rust server serves the
Vite build copied into `/app/web`.

## Auth/MCP settings

Auth/MCP settings are configured from `.env`:

- `NOTEGATE_AUTHGATE_URL=https://authgate.project-jelly.io`
- `NOTEGATE_PUBLIC_URL=http://localhost:9191`
- `NOTEGATE_OAUTH_CLIENT_ID=notegate-web`
- `NOTEGATE_MCP_OAUTH_CLIENT_ID=notegate-mcp`
- OAuth redirect URL defaults to `${NOTEGATE_PUBLIC_URL}/auth/callback`
- resource URL defaults to `${NOTEGATE_PUBLIC_URL}/mcp`
- `NOTEGATE_ENC_ROOT_SECRET` and `NOTEGATE_LOOKUP_ROOT_SECRET` must be at least 32 bytes.

First-time MCP users can open `${NOTEGATE_PUBLIC_URL}/auth/login` once to create the
local notegate user row, then reconnect the MCP client to `${NOTEGATE_PUBLIC_URL}/mcp`.

On startup, the API idempotently ensures missing active ENC/LOOKUP key epoch rows from
the configured root key IDs/secrets, then verifies them. If the database already has a
different active key for a domain, startup fails; rotation is not automatic.

## Checks

```sh
cargo fmt --all
cargo clippy --all-targets
cargo test
pnpm web:typecheck
pnpm web:test
pnpm web:build
```
