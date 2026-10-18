# syntax=docker/dockerfile:1

# --- 1. Frontend ---
FROM node:22-bookworm-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# --- 2. Backend (embeds frontend/dist via rust-embed) ---
FROM rust:1-bookworm AS backend
# rig -> rustls -> aws-lc-sys needs cmake (and a C compiler, already in the rust image)
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY backend/ backend/
COPY --from=frontend /app/frontend/dist frontend/dist
WORKDIR /app/backend
RUN cargo build --release -p server

# --- 3. Runtime ---
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data bento \
    && mkdir /data && chown bento:bento /data
COPY --from=backend /app/backend/target/release/server /usr/local/bin/server
USER bento
ENV DATABASE_URL=sqlite:///data/bento.db \
    APP_ENV=production \
    PORT=3000
VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:3000/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/server"]
