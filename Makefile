.PHONY: fmt check test clippy build release-check up curl-meta

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

release-check: fmt check test clippy build
	git diff --check

up:
	docker compose up --build -d

curl-meta:
	curl -fsS http://localhost:9191/health
	curl -fsS http://localhost:9191/ready
	curl -fsS http://localhost:9191/.well-known/oauth-authorization-server
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource
	curl -fsS http://localhost:9191/.well-known/oauth-protected-resource/mcp
	curl -i -sS http://localhost:9191/mcp -X POST -H 'content-type: application/json' -d '{}'
