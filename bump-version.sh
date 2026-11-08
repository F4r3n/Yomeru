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

# ── extension/package.json + package-lock.json ──────────────────────────────
jq --arg v "$VERSION" '.version = $v' extension/package.json > extension/package.json.tmp
mv extension/package.json.tmp extension/package.json
echo "  package.json   → $VERSION"

# The lockfile repeats the root package's version at the top and under
# packages[""]; npm rewrites both on the next install if they disagree.
jq --arg v "$VERSION" '.version = $v | .packages[""].version = $v' \
    extension/package-lock.json > extension/package-lock.json.tmp
mv extension/package-lock.json.tmp extension/package-lock.json
echo "  package-lock.json → $VERSION"

# ── Cargo.toml of every workspace member ─────────────────────────────────────
# Asked from cargo rather than listed by hand, so crates added later (the
# -core split, server, …) can't silently keep an old version.
mapfile -t CARGO_TOMLS < <(
    cargo metadata --no-deps --format-version 1 \
        | jq -r '.packages[].manifest_path' \
        | sed "s|^$PWD/||" \
        | sort
)
if [[ ${#CARGO_TOMLS[@]} -eq 0 ]]; then
    echo "Error: cargo metadata listed no workspace members" >&2
    exit 1
fi
for toml in "${CARGO_TOMLS[@]}"; do
    # Only the first `version = ` line: the [package] one, not a dependency's.
    sed -i "0,/^version = \"[^\"]*\"/s//version = \"$VERSION\"/" "$toml"
    echo "  $toml  → $VERSION"
done

# Record the new workspace versions in Cargo.lock now, so the bump commit
# includes it instead of the next build dirtying the tree.
cargo update --workspace --offline --quiet
echo "  Cargo.lock     → $VERSION"

echo ""
echo "Version bumped to $VERSION. Review the diff, then commit:"
echo "  jj diff --stat"
echo "  jj commit -m \"$VERSION\""
echo "  git tag \"v$VERSION\" \"\$(jj log -r '@-' --no-graph -T commit_id --no-pager)\""
