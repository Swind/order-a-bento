# syntax=docker/dockerfile:1

# --- 1. Frontend ---
# dist is arch-independent, so always build it on the build host's native platform
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# --- 2a. Rust toolchain base (runs on the build host's native arch; other target arches are cross-compiled, no QEMU) ---
# Everything here is a normal image layer so the GHA layer cache can store it
# (BuildKit cache mounts are not exported to the GHA cache).
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS rust-base
ARG BUILDARCH
ARG TARGETARCH
# rig -> rustls -> aws-lc-sys needs cmake (and a C compiler, already in the rust image)
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && if [ "$TARGETARCH" = "arm64" ] && [ "$BUILDARCH" != "arm64" ]; then \
         apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross; \
       fi \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.78
# Single source of truth for the Rust target triple and cross env, sourced by both the
# `cook` and the final `cargo build` so they cannot drift (a drift would void the dep cache).
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) RUST_TARGET=x86_64-unknown-linux-gnu ;; \
      arm64) RUST_TARGET=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "export RUST_TARGET=$RUST_TARGET" > /env.sh; \
    if [ "$TARGETARCH" = "arm64" ] && [ "$BUILDARCH" != "arm64" ]; then \
      echo "export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar" >> /env.sh; \
    fi; \
    . /env.sh; \
    rustup target add "$RUST_TARGET"
WORKDIR /app/backend

# --- 2b. Planner: dependency recipe (changes only when manifests/lockfile change) ---
FROM rust-base AS planner
COPY backend/ ./
RUN cargo chef prepare --recipe-path recipe.json

# --- 2c. Backend: cook dependencies (cached layer), then build the app ---
FROM rust-base AS backend
COPY --from=planner /app/backend/recipe.json recipe.json
# Same target/profile/package/env as the final build below. Does not need the frontend.
RUN . /env.sh && cargo chef cook --release --locked --target "$RUST_TARGET" -p server --recipe-path recipe.json
COPY backend/ ./
# Declared after the cook step so a new build number never invalidates the dependency cache.
ARG BUILD_NUMBER
# rust-embed embeds ../frontend/dist (relative to backend/crates/server) at compile time
COPY --from=frontend /app/frontend/dist /app/frontend/dist
RUN . /env.sh \
    && BUILD_NUMBER="$BUILD_NUMBER" cargo build --release --locked -p server --target "$RUST_TARGET" \
    && mkdir -p /out \
    && cp "target/$RUST_TARGET/release/server" /out/server

# --- 3. Runtime ---
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data bento \
    && mkdir /data && chown bento:bento /data
COPY --from=backend /out/server /usr/local/bin/server
USER bento
ENV DATABASE_URL=sqlite:///data/bento.db \
    APP_ENV=production \
    PORT=3000
VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:3000/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/server"]
