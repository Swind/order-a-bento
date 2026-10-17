.DEFAULT_GOAL := help
SHELL := /bin/bash

.PHONY: help dev-backend dev-frontend gen-api check build e2e docker-build admin ai-eval

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

dev-backend: ## Run the backend with dev login enabled (SQLite at backend/data/dev.db)
	cd backend && DEV_LOGIN=true APP_ENV=development DATABASE_URL=sqlite://data/dev.db cargo run -p server

admin: ## Create a local admin without Google or the web UI: make admin EMAIL=you@example.com NAME="You" (prompts for the password)
	@test -n "$(EMAIL)" -a -n "$(NAME)" || { echo 'usage: make admin EMAIL=you@example.com NAME="Your Name"' >&2; exit 1; }
	cd backend && DATABASE_URL=$${DATABASE_URL:-sqlite://data/dev.db} cargo run -q -p server -- admin create --email "$(EMAIL)" --name "$(NAME)"

dev-frontend: ## Run the Vite dev server
	cd frontend && npm run dev

gen-api: ## Export OpenAPI from the backend and regenerate frontend types
	cd backend && cargo run -q -p server --bin export_openapi > ../frontend/openapi.json
	cd frontend && npm run gen:api

check: ## Run all CI checks (backend fmt/clippy/test, OpenAPI sync, frontend typecheck/test/build)
	cd backend && cargo fmt --all --check
	cd backend && cargo clippy --workspace --all-targets -- -D warnings
	cd backend && cargo test --workspace
	cd backend && cargo run -q -p server --bin export_openapi > ../frontend/openapi.json
	git diff --exit-code -- frontend/openapi.json
	cd frontend && npm ci --prefer-offline --no-audit --no-fund
	cd frontend && npm run gen:api
	cd frontend && npm run typecheck
	cd frontend && npm test -- --run
	cd frontend && npm run build

build: ## Build the frontend, then the release server binary (embeds frontend/dist)
	cd frontend && npm run build
	cd backend && cargo build --release -p server

e2e: build ## Playwright E2E against the release binary (local: CHROME_PATH=/usr/bin/google-chrome make e2e)
	cd e2e && npm ci --no-audit --no-fund && npx playwright test

docker-build: ## Build the Docker image (order-a-bento:latest)
	docker build -t order-a-bento:latest .

# Manual only (calls a real model, costs tokens): never part of `make check` or CI.
# Uses ANTHROPIC_API_KEY / ANTHROPIC_BASE_URL / AI_MODEL / AI_PROVIDER as the server does. When
# ANTHROPIC_API_KEY is unset, STRATA_API_KEY / STRATA_ANTHROPIC_BASE_URL / STRATA_MODEL (from the
# environment or .env) are mapped onto them with AI_PROVIDER=anthropic-compatible. Secrets are never printed.
# Extra flags: make ai-eval ARGS="--filter 03 --repeat 3"
ai-eval: ## Run the AI import eval (eval/cases) against the configured model; report in eval/report.json
	@set -a; [ -f .env ] && . ./.env; set +a; \
	if [ -z "$$ANTHROPIC_API_KEY" ] && [ -n "$$STRATA_API_KEY" ]; then \
		export ANTHROPIC_API_KEY="$$STRATA_API_KEY" ANTHROPIC_BASE_URL="$$STRATA_ANTHROPIC_BASE_URL" AI_MODEL="$$STRATA_MODEL" AI_PROVIDER="$${AI_PROVIDER:-anthropic-compatible}"; \
		echo "ai-eval: using STRATA_* settings (AI_PROVIDER=$$AI_PROVIDER)"; \
	fi; \
	cd backend && cargo run -q -p server --bin ai_eval -- --cases ../eval/cases --out ../eval/report.json $(ARGS)
