#!/bin/sh
# Entrypoint for the yomeru-dicts builder image. See xtask/Dockerfile.
#
# Behaviour:
#   - If /dicts already holds all three .bin files and FORCE != 1, exit
#     immediately. Lets `depends_on` keep `docker compose up` cheap
#     after the first run.
#   - Otherwise: run `cargo xtask build-all` (downloads JMdict_e,
#     kanjidic2, examples.utf if not present in /build), then copy the
#     fresh .bin files into /dicts so the server volume sees them.
set -eu

mkdir -p /dicts /build/extension/data

if [ "${FORCE:-0}" != "1" ] \
    && [ -s /dicts/jmdict.bin ] \
    && [ -s /dicts/kanjidic.bin ] \
    && [ -s /dicts/examples.bin ]; then
    echo "yomeru-dicts: /dicts already populated; skipping build. Set FORCE=1 to rebuild."
    exit 0
fi

cd /build
# build-dicts (not build-all) — we don't have wasm-pack or npm in
# this image, and the server only needs the three .bin files.
cargo run --release -p xtask -- build-dicts

cp extension/data/jmdict.bin   /dicts/jmdict.bin
cp extension/data/kanjidic.bin /dicts/kanjidic.bin
cp extension/data/examples.bin /dicts/examples.bin

echo "yomeru-dicts: wrote jmdict.bin / kanjidic.bin / examples.bin to /dicts."
