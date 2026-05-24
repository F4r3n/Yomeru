#!/usr/bin/env bash
# deploy.sh — build the website via Podman, place the bundle on disk,
# then build & launch yomeru-server through `podman compose`.

set -euo pipefail
cd "$(dirname "$0")"

REPO_ROOT="$(pwd)"
WEB_DIST_DIR="${WEB_DIST_DIR:-$REPO_ROOT/web-dist}"

# Ensure destination directory exists before sync
mkdir -p "$WEB_DIST_DIR"

echo "==> [1/3] building yomeru-web bundle via podman"
TMP_BUNDLE="$(mktemp -d)"
trap 'rm -rf "$TMP_BUNDLE"' EXIT

# Write bundle straight to host filesystem
podman build -f app/web/Dockerfile --target export \
    --output "type=local,dest=$TMP_BUNDLE" .

echo "==> [2/3] syncing bundle to $WEB_DIST_DIR (pruning stale assets)"
# Using rsync with trailing slash to copy contents, not the folder itself.
# --delete ensures old hashed build chunks get removed.
rsync -a --delete "$TMP_BUNDLE/" "$WEB_DIST_DIR/"

COMPOSE=(podman-compose -f server/docker-compose.yml)

if [[ "${FORCE_DICTS:-0}" == "1" ]]; then
    echo "==> [3/3] FORCE_DICTS=1 — rebuilding dict bins from scratch"

    # 1. Clear out the existing volume if it exists so the init container resets
    # (Alternatively, pass the env var down to the 'up' command if your entrypoint supports it)
    "${COMPOSE[@]}" down
    podman volume rm -f server_yomeru-data || true

    echo "==> [3/3] launching yomeru-server with fresh dicts"
    "${COMPOSE[@]}" up -d --build
else
    echo "==> [3/3] building & launching yomeru-server via podman Compose"
    "${COMPOSE[@]}" up -d --build
fi

echo
echo "Server is up. Useful follow-ups:"
echo "  Logs:    podman-compose -f server/docker-compose.yml logs -f yomeru-server"
echo "  Stop:    podman-compose -f server/docker-compose.yml down"
echo "  Status:  podman-compose -f server/docker-compose.yml ps"
