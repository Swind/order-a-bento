.DEFAULT_GOAL := help
SHELL := /bin/bash

.PHONY: version bump help dev-backend dev-frontend gen-api check build e2e docker-build admin ai-eval

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

version: ## Print the project version (source: backend/Cargo.toml)
	@scripts/version.sh get

bump: ## Set the version in Cargo.toml, Cargo.lock and the frontend: make bump V=0.2.0 (does not commit)
	@test -n "$(V)" || { echo 'usage: make bump V=X.Y.Z' >&2; exit 1; }
	scripts/version.sh bump "$(V)"

check: ## Run all CI checks (version sync, backend fmt/clippy/test, OpenAPI sync, frontend typecheck/test/build)
	scripts/version.sh check
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
# Uses AI_API_KEY / AI_BASE_URL / AI_MODEL / AI_PROVIDER / AI_MAX_TOKENS as the server does (legacy ANTHROPIC_API_KEY /
# ANTHROPIC_BASE_URL still work for the Anthropic providers). Two shortcuts map other sources onto them:
#   - AI_PROVIDER_FILE=provider.json  ({"base_url": ..., "api_key": ...}, optional "model", "provider",
#     "session_header") -> AI_BASE_URL / AI_API_KEY, AI_PROVIDER=openai-compatible by default. Set AI_MODEL
#     (or "model" in the file): make ai-eval AI_PROVIDER_FILE=provider.json AI_MODEL=deepseek-v4.1-flash
#   - when no key is set, STRATA_API_KEY / STRATA_ANTHROPIC_BASE_URL / STRATA_MODEL (environment or .env)
#     with AI_PROVIDER=anthropic-compatible.
# Values given on the make command line win over .env. Secrets are never printed.
# Default = core cases only, 1 run each (7 model runs, cheap). Extra flags: ARGS="--all" adds the easy
# extended cases (01, 03, 07); ARGS="--filter 03 --all --repeat 3"; report path: AI_REPORT=/tmp/x.json
AI_REPORT ?= ../eval/report.json
ai-eval: ## Run the core AI import eval cases (ARGS=--all for all) against the configured model; report in eval/report.json
	@cli_model="$$AI_MODEL"; cli_provider="$$AI_PROVIDER"; cli_max_tokens="$$AI_MAX_TOKENS"; \
	set -a; [ -f .env ] && . ./.env; set +a; \
	[ -n "$$cli_model" ] && AI_MODEL="$$cli_model"; [ -n "$$cli_provider" ] && AI_PROVIDER="$$cli_provider"; \
	[ -n "$$cli_max_tokens" ] && AI_MAX_TOKENS="$$cli_max_tokens"; \
	if [ -n "$(AI_PROVIDER_FILE)" ]; then \
		test -r "$(AI_PROVIDER_FILE)" || { echo "ai-eval: cannot read $(AI_PROVIDER_FILE)" >&2; exit 1; }; \
		eval "$$(jq -r '@sh "export AI_BASE_URL=\(.base_url) AI_API_KEY=\(.api_key) file_model=\(.model // "") file_provider=\(.provider // "") file_session=\(.session_header // "")"' "$(AI_PROVIDER_FILE)")" || exit 1; \
		export AI_PROVIDER="$${AI_PROVIDER:-$${file_provider:-openai-compatible}}" AI_MODEL="$${AI_MODEL:-$$file_model}"; \
		[ -n "$$file_session" ] && export AI_SESSION_HEADER="$${AI_SESSION_HEADER:-$$file_session}"; \
		unset file_model file_provider file_session; \
		echo "ai-eval: using $(AI_PROVIDER_FILE) (AI_PROVIDER=$$AI_PROVIDER, AI_MODEL=$$AI_MODEL)"; \
	elif [ -z "$$AI_API_KEY" ] && [ -z "$$ANTHROPIC_API_KEY" ] && [ -n "$$STRATA_API_KEY" ]; then \
		export ANTHROPIC_API_KEY="$$STRATA_API_KEY" ANTHROPIC_BASE_URL="$$STRATA_ANTHROPIC_BASE_URL" AI_MODEL="$$STRATA_MODEL" AI_PROVIDER="$${AI_PROVIDER:-anthropic-compatible}"; \
		echo "ai-eval: using STRATA_* settings (AI_PROVIDER=$$AI_PROVIDER)"; \
	fi; \
	cd backend && cargo run -q -p server --bin ai_eval -- --cases ../eval/cases --out $(AI_REPORT) $(ARGS)
