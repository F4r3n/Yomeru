# syntax=docker/dockerfile:1.7
#
# Build-only image for the yomeru-web static bundle.
#
# Produces `target/dx/yomeru-web/release/web/public/` — pure static
# files (HTML + JS glue + hashed .wasm) ready to drop behind any
# static host (see DEPLOY.md). Lets you build the website on a host
# that only has Docker, without installing Rust + dioxus-cli.
#
# Build context is the workspace root.
#
# Extract the bundle to ./web-dist on the host:
#
#   docker build -f app/web/Dockerfile --target export \
#       --output type=local,dest=./web-dist .
#
# Or build the full image and copy out manually:
#
#   docker build -f app/web/Dockerfile -t yomeru-web-build .
#   id=$(docker create yomeru-web-build) \
#       && docker cp "$id":/out ./web-dist \
#       && docker rm "$id"

# ---------- Build stage ----------
# Trixie (not bookworm) — the prebuilt dx binary is linked against
# glibc 2.39, which bookworm doesn't ship.
FROM rust:1-slim-trixie AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config build-essential libssl-dev ca-certificates \
        curl unzip binaryen \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown

# Prebuilt dx binary via the official installer — seconds vs. several
# minutes of `cargo install dioxus-cli` building from source.
RUN curl -sSL https://dioxus.dev/install.sh | bash
ENV PATH="/root/.cargo/bin:${PATH}"

# Strip DWARF debug info from the wasm. The wasm-opt dx auto-downloads
# crashes on DWARF sections it doesn't understand ("compile unit size
# was incorrect"); stripping at rustc time means wasm-opt sees a clean
# binary. Includes the workspace's existing wasm32 rustflags so they
# aren't lost when this env var replaces the config-file rustflags.
ENV CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+bulk-memory,+mutable-globals -C strip=debuginfo"

WORKDIR /build

# Workspace requires every member manifest present even though we only
# build yomeru-web. `.dockerignore` keeps `target/` etc. out of context.
COPY Cargo.toml Cargo.lock ./
COPY app ./app
COPY crates ./crates
COPY server ./server
COPY xtask ./xtask

# Cache the cargo registry + the workspace target dir across builds.
# Copy the bundle out of the cache mount before it's discarded.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    dx bundle --package yomeru-web --platform web --release \
 && cp -r target/dx/yomeru-web/release/web/public /out

# ---------- Export stage ----------
# `docker build --target export --output type=local,dest=./web-dist`
# copies the bundle straight to the host without a runtime image.
FROM scratch AS export
COPY --from=builder /out /
