# notegate

A Rust backend (Axum + Tokio + sqlx + PostgreSQL) for notegate, a personal
Markdown file-tree notes service authenticated through authgate.

## Layout

```
notegate/
├─ Cargo.toml              # workspace root (shared deps, lints, profiles)
├─ rust-toolchain.toml     # pinned to Rust 1.95.0
├─ backend/crates/
│  ├─ api/                 # bin: Axum server, routes, HTTP/MCP surfaces
│  ├─ service/             # business logic and command semantics
│  ├─ db/                  # sqlx pool, repositories, migrations
│  ├─ model/               # shared domain data types
│  └─ core/                # config, limits, validation, shared error type
├─ backend/Dockerfile      # cargo-chef multi-stage, non-root Debian runtime
└─ docker-compose.yml      # postgres + scaled api + proxy
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
curl localhost:9191/ready    # readiness (database + applied migrations)
```

Auth/MCP local defaults are configured in `.env.example`:

- `NOTEGATE_AUTHGATE_URL=https://authgate.project-jelly.io`
- `NOTEGATE_PUBLIC_URL=http://localhost:9191` (builds first-time `login_url`)
- `NOTEGATE_OAUTH_CLIENT_ID=notegate-web` (browser OAuth client)
- `NOTEGATE_MCP_OAUTH_CLIENT_ID=notegate-mcp` (MCP OAuth client)
- `NOTEGATE_OAUTH_REDIRECT_URL=http://localhost:9191/auth/callback`
- `NOTEGATE_RESOURCE_URL=http://localhost:9191/mcp` (MCP URL/audience)
- `NOTEGATE_BROWSER_SESSION_SECRET` must be at least 32 bytes; required for browser login sessions.

First-time MCP users can open `${NOTEGATE_PUBLIC_URL}/auth/login` once to create the local notegate user row, then reconnect the MCP client to `NOTEGATE_RESOURCE_URL`. MCP OAuth clients must request the advertised `resource` and use the registered `notegate-mcp` public client.

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

Brings up Postgres + one scaled API service behind a local nginx proxy:

- `proxy` -> `http://localhost:9191`
- `api` -> `scale: 2`, exposed only inside the compose network

Both API replicas use the same `notegate-api` image and run the purge worker. Postgres advisory locking guarantees only one active purge transaction per database. Browser OAuth and MCP use the canonical `http://localhost:9191` public URL through the proxy.
