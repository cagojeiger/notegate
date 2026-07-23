.PHONY: fmt check test clippy build frontend-check release-check dev-db dev-infra web-build up logs curl-meta

fmt:
	cargo fmt --all --check

check:
	cargo check --workspace --all-targets

test:
	cargo test --workspace

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

build:
	cargo build --release --bin notegate-api

frontend-check:
	pnpm --filter web check:contrast
	pnpm --filter web typecheck
	pnpm --filter web lint
	pnpm --filter web test
	pnpm --filter web build

release-check: fmt check test clippy build frontend-check
	git diff --check

dev-db:
	docker compose up -d postgres

dev-infra:
	docker compose up -d --wait postgres minio
	docker compose run --rm --no-deps minio-init

web-build:
	docker compose build web

up:
	docker compose up --build -d --remove-orphans

logs:
	docker compose logs -f web proxy minio

curl-meta:
	curl -fsS http://localhost:9191/health
	curl -fsS http://localhost:9191/ready
	curl -fsS http://localhost:9191/.well-known/oauth-authorization-server
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource/mcp
	curl -i -sS http://localhost:9191/mcp -X POST -H 'content-type: application/json' -d '{}'
