#!/usr/bin/env bash
# deploy.sh — build the website via Podman, place the bundle on disk,
# then build & launch yomeru-server through `podman compose`.
#
# Steps:
#   1. podman build the yomeru-web static bundle.
#   2. rsync --delete the bundle into $WEB_DIST_DIR so stale hashed
#      assets from earlier builds get pruned.
#   3. podman compose up -d --build for yomeru-server (which depends
#      on the one-shot yomeru-dicts builder service).
#
# Env vars:
#   WEB_DIST_DIR  — destination for the static bundle (default
#                   $REPO_ROOT/web-dist). nginx (or whatever fronts
#                   the site) should be pointed here.
#   FORCE_DICTS   — when set to 1, rebuilds the dict bins from
#                   scratch before bringing the server up. The dicts
#                   live on a podman volume and the builder service
#                   skips itself when they're already present, so this
#                   is only needed when refreshing JMdict / KANJIDIC /
#                   examples from upstream.
#
# The server's runtime config (SMTP, ports, DB path inside the
# container) lives in server/.env — see server/.env.example.
#
# Dict data (jmdict.bin / kanjidic.bin / examples.bin) is handled
# automatically by the `yomeru-dicts` builder service in
# server/podman-compose.yml: yomeru-server `depends_on` it with
# `service_completed_successfully`, so the first `up` populates the
# shared dict volume before the server starts.

set -euo pipefail
cd "$(dirname "$0")"

REPO_ROOT="$(pwd)"
WEB_DIST_DIR="${WEB_DIST_DIR:-$REPO_ROOT/web-dist}"

echo "==> [1/3] building yomeru-web bundle via podman"
TMP_BUNDLE="$(mktemp -d)"
trap 'rm -rf "$TMP_BUNDLE"' EXIT
# `--target export --output type=local` writes the bundle straight to
# the host filesystem; the export stage is FROM scratch so no
# intermediate runtime image is created.
podman build -f app/web/Dockerfile --target export \
    --output "type=local,dest=$TMP_BUNDLE" .

echo "==> [2/3] copying bundle to $WEB_DIST_DIR"
mkdir -p "$WEB_DIST_DIR"
rsync -a --delete "$TMP_BUNDLE/" "$WEB_DIST_DIR/"

COMPOSE=(podman-compose -f server/docker-compose.yml)

if [[ "${FORCE_DICTS:-0}" == "1" ]]; then
    echo "==> [3/3] FORCE_DICTS=1 — rebuilding dict bins from scratch"
    # Standalone run, FORCE=1 overrides the entrypoint's skip-if-present
    # check. --build picks up any change to xtask/podmanfile.
    "${COMPOSE[@]}" run --rm --build -e FORCE=1 yomeru-dicts
    echo "==> [3/3] launching yomeru-server"
    "${COMPOSE[@]}" up -d --build yomeru-server
else
    echo "==> [3/3] building & launching yomeru-server via podman Compose"
    # `up -d --build` rebuilds images, runs the one-shot yomeru-dicts
    # builder (skips itself if the dict volume is already populated),
    # then starts yomeru-server detached. First run is ~5-10 min while
    # dicts download + index; subsequent runs are seconds.
    "${COMPOSE[@]}" up -d --build
fi

echo
echo "Server is up. Useful follow-ups:"
echo "  Logs:    podman-compose -f server/docker-compose.yml logs -f yomeru-server"
echo "  Stop:    podman-compose -f server/docker-compose.yml down"
echo "  Status:  podman-compose -f server/docker-compose.yml ps"
