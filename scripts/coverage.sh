#!/usr/bin/env bash
# Test coverage of the same runs as `cargo xtask test`, merged into one report.
#
#   ./scripts/coverage.sh          summary table in the terminal
#   ./scripts/coverage.sh --html   HTML report in target/llvm-cov/html
#   ./scripts/coverage.sh --open   HTML report, opened in a browser
#
# Needs: cargo install cargo-llvm-cov && rustup component add llvm-tools-preview
set -euo pipefail

case "${1-}" in
    "")     REPORT=(--summary-only) ;;
    --html) REPORT=(--html) ;;
    --open) REPORT=(--open) ;;
    *)
        echo "Usage: $0 [--html|--open]" >&2
        exit 1
        ;;
esac

if ! command -v cargo-llvm-cov >/dev/null; then
    echo "Error: cargo-llvm-cov not found." >&2
    echo "Install: cargo install cargo-llvm-cov && rustup component add llvm-tools-preview" >&2
    exit 1
fi

cd "$(dirname "$0")/.."

# Crates that only build for WASM or Android.
EXCLUDE=(jmdict-wasm srs-wasm kanjidic-wasm examples-wasm yomeru-android yomeru-extension)
WORKSPACE=(--workspace)
for krate in "${EXCLUDE[@]}"; do
    WORKSPACE+=(--exclude "$krate")
done

cargo llvm-cov clean --workspace
cargo llvm-cov --no-report "${WORKSPACE[@]}"
# The `full` feature compiles in jmdict-build code paths the default run
# never sees; collecting it separately lets the report merge both.
cargo llvm-cov --no-report -p jmdict-build --features full
cargo llvm-cov report "${REPORT[@]}"
