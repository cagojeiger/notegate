# notegate

Monorepo: a Rust backend (Axum + Tokio + sqlx + PostgreSQL) with a Next.js
frontend to follow.

## Layout

```
notegate/
├─ Cargo.toml              # workspace root (shared deps, lints, profiles)
├─ rust-toolchain.toml     # pinned to Rust 1.95.0
├─ backend/crates/
│  ├─ api/                 # bin: Axum server, routes, HTTP errors
│  ├─ domain/              # pure business logic (no HTTP / no sqlx)
│  ├─ db/                  # sqlx pool + migrations
│  └─ core/                # config + shared error type
├─ backend/Dockerfile      # cargo-chef multi-stage, non-root Debian runtime
├─ docker-compose.yml      # postgres + api (hardened)
└─ frontend/               # Next.js (later)
```

## Local development

```sh
# 1. Start Postgres
docker compose up -d postgres

# 2. Configure env
cp .env.example .env

# 3. Run the API (migrations run on startup)
cargo run --bin notegate-api
```

Health checks:

```sh
curl localhost:9191/health   # liveness
curl localhost:9191/ready    # readiness (pings the database)
```

Auth/MCP local defaults are configured in `.env.example`:

- `NOTEGATE_AUTHGATE_URL=https://authgate.project-jelly.io`
- `NOTEGATE_PUBLIC_URL=http://localhost:9191` (builds first-time `login_url`)
- `NOTEGATE_OAUTH_CLIENT_ID=notegate-web`
- `NOTEGATE_OAUTH_REDIRECT_URL=http://localhost:9191/callback`
- `NOTEGATE_RESOURCE_URL=http://localhost:9191/mcp` (MCP URL/audience)

First-time MCP users must open `${NOTEGATE_PUBLIC_URL}/login` once to create the
local notegate user row, then reconnect the MCP client to `NOTEGATE_RESOURCE_URL`.

## Checks

```sh
cargo fmt --all
cargo clippy --all-targets
cargo test
```

## Full stack via Docker

```sh
docker compose up --build
```
