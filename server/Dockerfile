# syntax=docker/dockerfile:1.7
#
# Multi-stage build for the yomeru-server binary.
# Build context: the workspace root (this file lives in `server/`).
#   docker build -f server/Dockerfile -t yomeru-server .

# ---------- Build stage ----------
FROM rust:1-slim-bookworm AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config build-essential \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy the full workspace. The workspace Cargo.toml lists every member, so
# every member manifest must be present even though `cargo build -p server`
# only compiles `server` and its transitive deps. `.dockerignore` keeps the
# context small.
COPY Cargo.toml Cargo.lock ./
COPY server ./server
COPY crates ./crates
COPY xtask ./xtask

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p server --bin yomeru-server \
 && cp /build/target/release/yomeru-server /usr/local/bin/yomeru-server

# ---------- Runtime stage ----------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 yomeru \
 && mkdir -p /data && chown yomeru:yomeru /data

COPY --from=builder /usr/local/bin/yomeru-server /usr/local/bin/yomeru-server

USER yomeru
WORKDIR /home/yomeru

# Defaults; override via env file or `-e`.
ENV YOMERU_PORT=8080 \
    YOMERU_DB_PATH=/data/yomeru.db

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/yomeru-server"]
