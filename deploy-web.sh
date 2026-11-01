#!/usr/bin/env bash
# deploy-web.sh — publish the website bundle built by deploy.sh to the
# directory nginx serves. Usually run with sudo, since the web root is
# not writable by the deploying user.
#
#   sudo ./deploy-web.sh                    # → /var/www/html/yomeru
#   sudo ./deploy-web.sh /srv/yomeru/web    # another web root
#
# Takes the destination as an argument rather than an env var because
# sudo drops the caller's environment.

set -euo pipefail
cd "$(dirname "$0")"

SRC="$(pwd)/web-dist"
DEST="${1:-/var/www/html/yomeru}"

if [[ ! -f "$SRC/index.html" ]]; then
    echo "deploy-web.sh: no bundle at $SRC — run ./deploy.sh first (without sudo)" >&2
    exit 1
fi

mkdir -p "$DEST"
echo "==> publishing $SRC → $DEST (pruning stale assets)"
# Files end up owned by whoever runs this (root under sudo) and readable
# by everyone, so nginx can serve them whatever user it runs as.
rsync -rt --delete --chmod=D755,F644 "$SRC/" "$DEST/"

echo "Website published to $DEST."
