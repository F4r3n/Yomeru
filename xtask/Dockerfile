# syntax=docker/dockerfile:1.7
#
# One-shot builder for the binary dictionary indexes. Runs `cargo xtask
# build-all` and writes the three .bin files into /dicts — a named
# volume shared with `yomeru-server` via docker-compose.yml.
#
# Lets you refresh the dictionaries without rebuilding the server
# image or touching the SQLite volume. Triggered manually with
#
#   docker compose run --rm yomeru-dicts          # build if missing
#   docker compose run --rm -e FORCE=1 yomeru-dicts  # rebuild from scratch
#
# `docker compose up -d` also runs this once (via depends_on) before
# starting the server, so first-time setup populates the volume.
#
# Build context is the workspace root.

FROM rust:1-slim-bookworm

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config build-essential libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Workspace manifests must all be present for cargo to load the
# workspace; `.dockerignore` keeps the context small.
COPY Cargo.toml Cargo.lock ./
COPY xtask ./xtask
COPY crates ./crates
COPY server ./server
COPY app ./app

# Warm the build cache so runtime startup is fast: pre-compile xtask
# and the three -build crates it shells out to. The build itself
# (and the upstream downloads) happens at container start, not image
# build, so dict refreshes never touch the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release -p xtask \
                          -p jmdict-build \
                          -p kanjidic-build \
                          -p examples-build

COPY xtask/docker-entrypoint.sh /usr/local/bin/yomeru-dicts-build
RUN chmod +x /usr/local/bin/yomeru-dicts-build

VOLUME ["/dicts"]
ENTRYPOINT ["/usr/local/bin/yomeru-dicts-build"]
