#!/bin/sh
# Builds the full dictionary the reports need (jmdict-types/full), once.
set -e
c=$HOME/.cache/yomeru-sentence
mkdir -p "$c"
cd "$(dirname "$0")/../../.."
[ -f JMdict_e ] || cargo xtask download-dict
[ -f examples.utf ] || cargo xtask download-examples
cargo run -q --release -p jmdict-build --features full -- --input JMdict_e --output "$c/jmdict-full.bin"
ls -la "$c/jmdict-full.bin"
