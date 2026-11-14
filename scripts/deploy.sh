#!/usr/bin/env bash
# deploy.sh — build the website bundle into ./web-dist via Podman, then
# build & launch yomeru-server through `podman compose`.
#
# Run as your normal user, not with sudo: rootless Podman keeps its
# images and volumes per user, so a root run would start a separate
# server with empty volumes. Publishing the bundle to the web root is
# done afterwards by deploy-web.sh, which is the part that needs sudo.
#
# Before the new server starts (and runs its schema migrations), the live
# SQLite database is backed up to ./backups. Restoring that file is the only
# clean way back from a bad server release, since old server code can't read
# a migrated database. After restoring one, rotate the delta-sync epoch before
# the server takes traffic, so every client falls back to a full sync instead
# of trusting a cursor the restored database never reached:
#   sqlite3 yomeru.db "UPDATE sync_meta SET epoch = lower(hex(randomblob(8)))"
#
#   ./scripts/deploy.sh                  # backup + bundle + server
#   FORCE_DICTS=1 ./scripts/deploy.sh    # ... and rebuild the dict bins
#   SKIP_DB_BACKUP=1 ./scripts/deploy.sh # deploy without a backup (not recommended)
#   KEEP_BACKUPS=20 ./scripts/deploy.sh  # how many backups to keep (default 10)

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ $EUID -eq 0 ]]; then
    echo "deploy.sh: run without sudo (rootless Podman is per user); then: sudo ./scripts/deploy-web.sh" >&2
    exit 1
fi

REPO_ROOT="$(pwd)"
WEB_DIST_DIR="${WEB_DIST_DIR:-$REPO_ROOT/web-dist}"
COMPOSE=(podman-compose -f server/docker-compose.yml)
BACKUP_DIR="${BACKUP_DIR:-$REPO_ROOT/backups}"
KEEP_BACKUPS="${KEEP_BACKUPS:-10}"
CONTAINER=yomeru-server
DB_PATH=/data/yomeru.db

# Online backup of the live database through SQLite's backup API, run inside
# the server container (its image ships sqlite3). Safe while the server is
# writing, unlike copying the file, and it folds in the WAL. The copy is
# integrity-checked before it's kept.
backup_db() {
    if [[ "${SKIP_DB_BACKUP:-0}" == "1" ]]; then
        echo "    SKIP_DB_BACKUP=1 — not backing up the database"
        return
    fi
    if ! podman container exists "$CONTAINER"; then
        echo "    no $CONTAINER container yet (first deploy?) — nothing to back up"
        return
    fi
    if [[ "$(podman inspect -f '{{.State.Running}}' "$CONTAINER")" != "true" ]]; then
        echo "deploy.sh: $CONTAINER exists but isn't running, so its database can't be backed up." >&2
        echo "  Start it (${COMPOSE[*]} up -d $CONTAINER) and retry, or rerun with SKIP_DB_BACKUP=1." >&2
        exit 1
    fi

    local stamp tmp dest
    stamp="$(date +%Y%m%d-%H%M%S)"
    tmp="/tmp/yomeru-backup-$stamp.db"
    dest="$BACKUP_DIR/yomeru-$stamp.db"
    mkdir -p "$BACKUP_DIR"

    podman exec "$CONTAINER" sqlite3 "$DB_PATH" ".backup '$tmp'"
    local check
    check="$(podman exec "$CONTAINER" sqlite3 "$tmp" 'PRAGMA integrity_check')"
    if [[ "$check" != "ok" ]]; then
        podman exec "$CONTAINER" rm -f "$tmp" || true
        echo "deploy.sh: backup failed its integrity check, aborting: $check" >&2
        exit 1
    fi
    podman cp "$CONTAINER:$tmp" "$dest"
    podman exec "$CONTAINER" rm -f "$tmp"
    chmod 600 "$dest" # holds every user's email and session hashes
    echo "    saved $dest ($(du -h "$dest" | cut -f1))"

    # Keep the newest $KEEP_BACKUPS; the timestamped names sort by age.
    local old
    old="$(ls -1 "$BACKUP_DIR"/yomeru-*.db 2>/dev/null | sort | head -n -"$KEEP_BACKUPS")"
    if [[ -n "$old" ]]; then
        echo "$old" | xargs rm -f
        echo "    pruned $(echo "$old" | wc -l) old backup(s), keeping $KEEP_BACKUPS"
    fi
}

mkdir -p "$WEB_DIST_DIR"

echo "==> [1/4] building yomeru-web bundle via podman"
TMP_BUNDLE="$(mktemp -d)"
trap 'rm -rf "$TMP_BUNDLE"' EXIT

podman build -f app/web/Dockerfile --target export \
    --output "type=local,dest=$TMP_BUNDLE" .

echo "==> [2/4] syncing bundle to $WEB_DIST_DIR (pruning stale assets)"
# Trailing slash copies the contents, not the folder; --delete drops old
# hashed chunks.
rsync -a --delete "$TMP_BUNDLE/" "$WEB_DIST_DIR/"

echo "==> [3/4] backing up the database to $BACKUP_DIR"
# Right before the swap, so the backup misses as few writes as possible.
# The new server may migrate the schema on startup; this is the way back.
backup_db

if [[ "${FORCE_DICTS:-0}" == "1" ]]; then
    # Before the server swap, with the new builder: when a dict format
    # changes, the new server can't read the old bins and would crash-loop.
    # The running server loaded its dicts at startup, so rewriting them
    # under it is harmless; it's recreated on the new ones right after.
    echo "==> FORCE_DICTS=1 — rebuilding dict bins with the new builder"
    "${COMPOSE[@]}" build yomeru-dicts
    "${COMPOSE[@]}" run --rm -e FORCE=1 yomeru-dicts
fi

echo "==> [4/4] building & launching yomeru-server via podman compose"
# Builds both images. --force-recreate because podman-compose only
# recreates a container when docker-compose.yml changed, not its image:
# without it the old server keeps running. Volumes are kept. The dict
# builder skips its work while the dict volume already holds the bins,
# so without FORCE_DICTS=1 the new server starts on the old ones.
"${COMPOSE[@]}" up -d --build --force-recreate

# podman-compose can leave the server in "Created" without ever starting it:
# it mishandles `depends_on: condition: service_completed_successfully` once
# the one-shot dict builder has exited. The builder has finished by now
# (`up -d` waits for it), so start the server explicitly. A no-op if it's
# already running.
podman start "$CONTAINER" >/dev/null
# A server that exits at startup is restarted by `restart: unless-stopped`,
# so a single "Running" sample can catch it between crashes. Wait a bit and
# also require that it hasn't been restarted.
sleep 5
if [[ "$(podman inspect -f '{{.State.Running}}' "$CONTAINER")" != "true" \
    || "$(podman inspect -f '{{.RestartCount}}' "$CONTAINER")" != "0" ]]; then
    podman stop "$CONTAINER" >/dev/null 2>&1 || true
    echo "deploy.sh: $CONTAINER failed to start (stopped it). Last log lines:" >&2
    podman logs --tail 30 "$CONTAINER" >&2
    if podman logs --tail 30 "$CONTAINER" 2>&1 | grep -qi "unsupported.*version"; then
        echo >&2
        echo "  The new server can't read the dict bins on the volume (their format changed)." >&2
        echo "  Rerun with FORCE_DICTS=1 to rebuild them before the server starts, plus" >&2
        echo "  SKIP_DB_BACKUP=1: the server is down, so the backup this run took is current." >&2
    fi
    exit 1
fi

echo
echo "Server is up. Now publish the website:"
echo "  sudo ./scripts/deploy-web.sh"
echo
echo "Useful follow-ups:"
echo "  Logs:    ${COMPOSE[*]} logs -f yomeru-server"
echo "  Stop:    ${COMPOSE[*]} down"
echo "  Status:  ${COMPOSE[*]} ps"
echo "  Backups: ls $BACKUP_DIR"
