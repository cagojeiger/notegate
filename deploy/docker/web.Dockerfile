# syntax=docker/dockerfile:1.7

# Stage 1: shared Rust build toolbox.
#
# This stage installs system build tools, cargo-chef, and sccache once. The
# later planner/builder stages inherit from it so dependency planning and final
# compilation use the exact same Rust/toolchain environment.
#
# We use Debian instead of Alpine to avoid musl-specific surprises and to keep
# Rust crate builds close to the runtime libc.
FROM rust:1.99.1-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29 AS chef

WORKDIR /app
# Native toolchain for crates with C build scripts, plus certificates for Cargo HTTPS.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        cmake \
        g++ \
        make \
        perl \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy rust-src
RUN --mount=type=cache,id=notegate-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=notegate-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-chef --version 0.1.78 --locked \
    && cargo install sccache --version 0.18.0 --locked --no-default-features

ENV RUSTC_WRAPPER=sccache \
    SCCACHE_DIR=/sccache \
    CARGO_INCREMENTAL=0

# Stage 2: dependency graph planner.
#
# cargo-chef reads manifests/source layout and emits recipe.json. The recipe
# changes only when dependency-relevant inputs change, so Docker can reuse the
# expensive dependency build layer when application code changes.
FROM chef AS planner

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY backend ./backend
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: dashboard build.
#
# The final runtime image serves this Vite build from the Rust server, so the
# deployed `web` container contains both the dashboard and the API/MCP backend.
FROM node:22.23.3-bookworm-slim@sha256:c3de60bf2f9dd0ac6370e6117950ff62d6e339527e7472301c9c78a017978392 AS web-builder

WORKDIR /app
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY frontend/web/package.json ./frontend/web/package.json
RUN --mount=type=cache,id=notegate-pnpm-store,target=/pnpm/store,sharing=locked \
    pnpm install --frozen-lockfile
COPY VERSION ./VERSION
COPY frontend ./frontend
RUN pnpm --filter web build

# Stage 4: dependency cache + application build.
#
# First cook dependencies from recipe.json, then copy the real source and build
# the notegate-api binary. Keep Cargo registry/git data and target artifacts in
# the dependency layer so the GitHub Actions cache can restore them on fresh
# builders. The sccache mount remains a local build accelerator.
# Main CI can warm this target without building the application or frontend.
FROM chef AS dependencies

COPY --from=planner /app/recipe.json recipe.json
# Queries are runtime-checked (no sqlx `query!`/`query_as!` macros), so the build
# needs neither offline `.sqlx` metadata nor SQLX_OFFLINE / a live DB.
RUN --mount=type=cache,id=notegate-sccache,target=/sccache,sharing=locked \
    cargo chef cook --release --locked --bin notegate-api --recipe-path recipe.json

FROM dependencies AS builder

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY backend ./backend
COPY frontend/web/src/design/theme.css frontend/web/src/design/auth.css ./frontend/web/src/design/
COPY frontend/web/public/brand/svg/logo-horizontal-light.svg frontend/web/public/brand/svg/logo-horizontal-dark.svg ./frontend/web/public/brand/svg/
RUN --mount=type=cache,id=notegate-sccache,target=/sccache,sharing=locked \
    cargo build --release --locked --bin notegate-api \
    && cp /app/target/release/notegate-api /usr/local/bin/notegate-api

# Stage 5: shared minimal runtime image.
#
# The split-topology test target uses this image without dashboard assets, so
# backend process composition can be built independently of the Node stage.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:e792ab3d241a468a4fd7519ddbbebe66b49b5f365771716ea688ad40b6c6f1c2 AS runtime-base

# Distroless supplies glibc, libgcc and CA certificates without a shell or
# package manager. Keep the existing numeric identity for mounted volumes.
WORKDIR /app
COPY --from=builder /usr/local/bin/notegate-api /usr/local/bin/notegate-api

ENV NOTEGATE_BIND_ADDR=0.0.0.0:9191

USER 10001:10001
ENTRYPOINT ["/usr/local/bin/notegate-api"]

# Backend-only image used by docker-compose.split.yml.
FROM runtime-base AS runtime-headless
EXPOSE 9191 9192

# Production image additionally serves the built dashboard.
FROM runtime-base AS runtime

COPY --from=web-builder /app/frontend/web/dist /app/web
ENV NOTEGATE_WEB_DIST_DIR=/app/web
EXPOSE 9191
