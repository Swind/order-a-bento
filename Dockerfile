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
# Litestream (optional SQLite replication, enabled by LITESTREAM_BUCKET; see deploy/entrypoint.sh)
ARG TARGETARCH
ARG LITESTREAM_VERSION=0.3.13
ARG LITESTREAM_SHA256_AMD64=eb75a3de5cab03875cdae9f5f539e6aedadd66607003d9b1e7a9077948818ba0
ARG LITESTREAM_SHA256_ARM64=9585f5a508516bd66af2b2376bab4de256a5ef8e2b73ec760559e679628f2d59
RUN set -eu; \
    case "${TARGETARCH:-amd64}" in \
      amd64) sha="$LITESTREAM_SHA256_AMD64" ;; \
      arm64) sha="$LITESTREAM_SHA256_ARM64" ;; \
      *) echo "unsupported arch ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/litestream.tgz "https://github.com/benbjohnson/litestream/releases/download/v${LITESTREAM_VERSION}/litestream-v${LITESTREAM_VERSION}-linux-${TARGETARCH:-amd64}.tar.gz"; \
    echo "${sha}  /tmp/litestream.tgz" | sha256sum -c -; \
    tar -xzf /tmp/litestream.tgz -C /usr/local/bin litestream; \
    rm /tmp/litestream.tgz
COPY --from=backend /app/backend/target/release/server /usr/local/bin/server
COPY deploy/litestream.yml /etc/litestream.yml
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
USER bento
ENV DATABASE_URL=sqlite:///data/bento.db \
    APP_ENV=production \
    PORT=3000
VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:3000/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["server"]
