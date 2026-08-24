.PHONY: fmt check test clippy build cli-build frontend-check release-check dev-db dev-infra web-build up logs curl-meta curl-metrics split-up split-test split-test-isolation split-logs split-down

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

cli-build:
	cargo build --release --bin notegate-cli

frontend-check:
	pnpm audit --prod --audit-level moderate
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
	docker compose logs -f web proxy minio prometheus grafana

curl-meta:
	curl -fsS http://localhost:9191/health
	curl -fsS http://localhost:9191/ready
	curl -fsS http://localhost:9191/.well-known/oauth-authorization-server
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource/mcp
	curl -i -sS http://localhost:9191/mcp -X POST -H 'content-type: application/json' -d '{}'

curl-metrics:
	curl -fsS http://localhost:9191/metrics
	curl -fsS http://localhost:9090/-/ready
	curl -fsS http://localhost:3000/api/health

split-up:
	docker compose -f docker-compose.split.yml build api
	docker compose -f docker-compose.split.yml up -d --remove-orphans

split-test: split-up
	docker compose -f docker-compose.split.yml --profile test run --rm --no-deps smoke

split-test-isolation: split-test
	@set -eu; \
	cleanup() { docker compose -f docker-compose.split.yml start api >/dev/null; }; \
	trap cleanup EXIT HUP INT TERM; \
	docker compose -f docker-compose.split.yml stop api; \
	docker compose -f docker-compose.split.yml --profile test run --rm --no-deps smoke-isolation; \
	cleanup; \
	trap - EXIT HUP INT TERM; \
	docker compose -f docker-compose.split.yml --profile test run --rm --no-deps smoke

split-logs:
	docker compose -f docker-compose.split.yml logs -f api search worker reconciler prometheus grafana

split-down:
	docker compose -f docker-compose.split.yml down --remove-orphans
