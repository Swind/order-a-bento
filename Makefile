.DEFAULT_GOAL := help
SHELL := /bin/bash

.PHONY: help dev-backend dev-frontend gen-api check build e2e docker-build

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

dev-backend: ## Run the backend with dev login enabled (SQLite at backend/data/dev.db)
	cd backend && DEV_LOGIN=true APP_ENV=development DATABASE_URL=sqlite://data/dev.db cargo run -p server

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
