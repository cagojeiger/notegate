# syntax=docker/dockerfile:1.7

# Stage 1: shared Rust build toolbox.
#
# This stage installs system build tools, cargo-chef, and sccache once. The
# later planner/builder stages inherit from it so dependency planning and final
# compilation use the exact same Rust/toolchain environment.
#
# We use Debian instead of Alpine to avoid musl-specific surprises and to keep
# Rust crate builds close to the runtime libc.
FROM rust:1.95.0-bookworm AS chef

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
    cargo install cargo-chef --locked \
    && cargo install sccache --locked --no-default-features

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
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY frontend/web/package.json ./frontend/web/package.json
RUN --mount=type=cache,id=notegate-pnpm-store,target=/pnpm/store,sharing=locked \
    pnpm install --frozen-lockfile
COPY frontend ./frontend
RUN pnpm --filter web build

# Stage 4: dependency cache + application build.
#
# First cook dependencies from recipe.json, then copy the real source and build
# the notegate-api binary. BuildKit cache mounts keep Cargo registry/git data,
# sccache objects, and target artifacts outside the image layers but reusable
# across local Docker builds.
FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
# Queries are runtime-checked (no sqlx `query!`/`query_as!` macros), so the build
# needs neither offline `.sqlx` metadata nor SQLX_OFFLINE / a live DB.
RUN --mount=type=cache,id=notegate-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=notegate-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=notegate-sccache,target=/sccache,sharing=locked \
    --mount=type=cache,id=notegate-target-release,target=/app/target,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY backend ./backend
RUN --mount=type=cache,id=notegate-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=notegate-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=notegate-sccache,target=/sccache,sharing=locked \
    --mount=type=cache,id=notegate-target-release,target=/app/target,sharing=locked \
    cargo build --release --locked --bin notegate-api \
    && cp /app/target/release/notegate-api /usr/local/bin/notegate-api

# Stage 5: minimal runtime image.
#
# The final image contains Debian slim, the CA bundle, a non-root user, the
# compiled backend, and the built dashboard assets.
FROM debian:bookworm-slim AS runtime

# Runtime only needs a CA bundle for outbound HTTPS. Copy it from the build
# image instead of apt-installing ca-certificates, which would also pull
# openssl packages into the final image.
COPY --from=chef /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN groupadd --gid 10001 app \
    && useradd --uid 10001 --gid app --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin appuser
WORKDIR /app
COPY --from=builder /usr/local/bin/notegate-api /usr/local/bin/notegate-api
COPY --from=web-builder /app/frontend/web/dist /app/web

ENV NOTEGATE_BIND_ADDR=0.0.0.0:9191 \
    NOTEGATE_WEB_DIST_DIR=/app/web
EXPOSE 9191

USER appuser
ENTRYPOINT ["/usr/local/bin/notegate-api"]
