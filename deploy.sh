#!/usr/bin/env bash
# deploy.sh — build the website bundle into ./web-dist via Podman, then
# build & launch yomeru-server through `podman compose`.
#
# Run as your normal user, not with sudo: rootless Podman keeps its
# images and volumes per user, so a root run would start a separate
# server with empty volumes. Publishing the bundle to the web root is
# done afterwards by deploy-web.sh, which is the part that needs sudo.
#
#   ./deploy.sh                  # bundle + server
#   FORCE_DICTS=1 ./deploy.sh    # ... and rebuild the dict bins

set -euo pipefail
cd "$(dirname "$0")"

if [[ $EUID -eq 0 ]]; then
    echo "deploy.sh: run without sudo (rootless Podman is per user); then: sudo ./deploy-web.sh" >&2
    exit 1
fi

REPO_ROOT="$(pwd)"
WEB_DIST_DIR="${WEB_DIST_DIR:-$REPO_ROOT/web-dist}"
COMPOSE=(podman-compose -f server/docker-compose.yml)

mkdir -p "$WEB_DIST_DIR"

echo "==> [1/3] building yomeru-web bundle via podman"
TMP_BUNDLE="$(mktemp -d)"
trap 'rm -rf "$TMP_BUNDLE"' EXIT

podman build -f app/web/Dockerfile --target export \
    --output "type=local,dest=$TMP_BUNDLE" .

echo "==> [2/3] syncing bundle to $WEB_DIST_DIR (pruning stale assets)"
# Trailing slash copies the contents, not the folder; --delete drops old
# hashed chunks.
rsync -a --delete "$TMP_BUNDLE/" "$WEB_DIST_DIR/"

echo "==> [3/3] building & launching yomeru-server via podman compose"
# Builds both images. --force-recreate because podman-compose only
# recreates a container when docker-compose.yml changed, not its image:
# without it the old server keeps running. Volumes are kept. The dict
# builder skips its work while the dict volume already holds the bins,
# so the new server starts on the old ones.
"${COMPOSE[@]}" up -d --build --force-recreate

if [[ "${FORCE_DICTS:-0}" == "1" ]]; then
    # Done after the server update on purpose: a server only reads dict
    # formats it knows, so new dicts under an old server would stop it.
    echo "==> FORCE_DICTS=1 — rebuilding dict bins, then restarting the server"
    "${COMPOSE[@]}" run --rm -e FORCE=1 yomeru-dicts
    "${COMPOSE[@]}" restart yomeru-server
fi

echo
echo "Server is up. Now publish the website:"
echo "  sudo ./deploy-web.sh"
echo
echo "Useful follow-ups:"
echo "  Logs:    ${COMPOSE[*]} logs -f yomeru-server"
echo "  Stop:    ${COMPOSE[*]} down"
echo "  Status:  ${COMPOSE[*]} ps"
