#!/usr/bin/env bash
set -euo pipefail

VERSION="${1-}"
if [[ -z "$VERSION" ]]; then
    echo "Usage: $0 <version>  (e.g. 0.3.0)" >&2
    exit 1
fi
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: '$VERSION' is not a valid semver (expected X.Y.Z)" >&2
    exit 1
fi

cd "$(dirname "$0")"

# ── extension/manifest.json ───────────────────────────────────────────────────
jq --arg v "$VERSION" '.version = $v' extension/manifest.json > extension/manifest.json.tmp
mv extension/manifest.json.tmp extension/manifest.json
echo "  manifest.json  → $VERSION"

# ── extension/package.json ────────────────────────────────────────────────────
jq --arg v "$VERSION" '.version = $v' extension/package.json > extension/package.json.tmp
mv extension/package.json.tmp extension/package.json
echo "  package.json   → $VERSION"

# ── Cargo.toml files (workspace crates + xtask) ──────────────────────────────
CARGO_TOMLS=(
    xtask/Cargo.toml
    crates/japanese-utils/Cargo.toml
    crates/jmdict-types/Cargo.toml
    crates/jmdict-build/Cargo.toml
    crates/jmdict-wasm/Cargo.toml
    crates/kanjidic-types/Cargo.toml
    crates/kanjidic-build/Cargo.toml
    crates/kanjidic-wasm/Cargo.toml
    crates/deinflect/Cargo.toml
    crates/srs-core/Cargo.toml
    crates/srs-wasm/Cargo.toml
    crates/examples-types/Cargo.toml
    crates/examples-build/Cargo.toml
    crates/examples-wasm/Cargo.toml
)
for toml in "${CARGO_TOMLS[@]}"; do
    sed -i "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$toml"
    echo "  $toml  → $VERSION"
done

echo ""
echo "Version bumped to $VERSION. Review the diff, then commit:"
echo "  jj diff --stat"
echo "  jj commit -m \"$VERSION\""
echo "  git tag \"v$VERSION\" \"\$(jj log -r '@-' --no-graph -T commit_id --no-pager)\""
