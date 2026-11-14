#!/usr/bin/env bash
# Load-test yomeru-server endpoints with oha, one endpoint at a time.
#
#   ./scripts/bench-server.sh                    every endpoint, against a running server
#   ./scripts/bench-server.sh lookup kanji       only the named endpoints
#   ./scripts/bench-server.sh --start            build + start a dev server, bench, stop it
#
# Env: URL (default http://127.0.0.1:8080), DURATION (10s), CONNECTIONS (50).
# The server must run with --dev-mode --no-rate-limit, or most requests are 429s;
# `sync` also needs dev mode to get a token without OTP.
#
# Needs: cargo install oha; jq.
set -euo pipefail

URL="${URL:-http://127.0.0.1:8080}"
DURATION="${DURATION:-10s}"
CONNECTIONS="${CONNECTIONS:-50}"

cd "$(dirname "$0")/.."

for tool in oha jq curl; do
    if ! command -v "$tool" >/dev/null; then
        echo "Error: $tool not found." >&2
        exit 1
    fi
done

START=0
SELECTED=()
for arg in "$@"; do
    case "$arg" in
        --start) START=1 ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) SELECTED+=("$arg") ;;
    esac
done

if [[ $START == 1 ]]; then
    cargo build --release -p server
    TMP="$(mktemp -d)"
    PORT="${URL##*:}"
    ./target/release/yomeru-server --dev-mode --no-rate-limit --port "$PORT" \
        --db "$TMP/bench.db" --data-dir "${DATA_DIR:-extension/data}" \
        >"$TMP/server.log" 2>&1 &
    SERVER_PID=$!
    trap 'kill "$SERVER_PID" 2>/dev/null || true; rm -rf "$TMP"' EXIT
    echo "Started server (pid $SERVER_PID), log: $TMP/server.log"
fi

# Wait up to 30 s for the server: dict loading takes a moment.
for _ in $(seq 60); do
    if curl -fs -o /dev/null "$URL/api/privacy"; then break; fi
    sleep 0.5
done
if ! curl -fsS -o /dev/null "$URL/api/privacy"; then
    echo "Error: server not reachable at $URL" >&2
    [[ $START == 1 ]] && tail -20 "$TMP/server.log" >&2
    exit 1
fi

# 30 quick lookups exceed the 20/s quota, so a 429 means rate limiting is on.
for _ in $(seq 30); do
    code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
        -d '{"words":["食べる"]}' "$URL/api/lookup")"
    if [[ $code == 429 ]]; then
        echo "Warning: rate limiting is active; start the server with --dev-mode --no-rate-limit." >&2
        break
    fi
done

# name | method | path | body
ENDPOINTS=(
    'privacy|GET|/api/privacy|'
    'lookup|POST|/api/lookup|{"words":["食べる","食べ","食","日本語","日本","日"]}'
    'lookup-by-sequence|POST|/api/lookup-by-sequence|{"sequences":[1358280,1464530,1582710]}'
    'lookup-prefix|POST|/api/lookup-prefix|{"text":"たべ","max":30}'
    'kanji|POST|/api/kanji|{"word":"日本語"}'
    'kanji-parts|POST|/api/kanji-parts|'
    'kanji-by-parts|POST|/api/kanji-by-parts|{"parts":"口木","max":100}'
    'examples|POST|/api/examples|{"word":"食べる","max":5}'
    'sync|POST|/api/sync|{"cards":[],"deletions":[],"reviews":[]}'
)

wanted() {
    [[ ${#SELECTED[@]} == 0 ]] && return 0
    local s
    for s in "${SELECTED[@]}"; do [[ $s == "$1" ]] && return 0; done
    return 1
}

TOKEN=""
if wanted sync; then
    resp="$(curl -fsS -X POST -H 'Content-Type: application/json' \
        -d '{"email":"bench@example.com"}' "$URL/api/auth/request")" \
        || { echo "Error: /api/auth/request failed" >&2; exit 1; }
    TOKEN="$(jq -r '.token // empty' <<<"$resp")"
    [[ -z $TOKEN ]] && echo "Warning: no token (server not in dev mode?); sync will 401." >&2
fi

echo "Target $URL — $DURATION per endpoint, $CONNECTIONS connections"
printf '\n%-20s %10s %9s %9s %9s %9s  %s\n' endpoint req/s p50 p90 p99 max statuses
RESULTS="$(mktemp)"
for entry in "${ENDPOINTS[@]}"; do
    IFS='|' read -r name method path body <<<"$entry"
    wanted "$name" || continue

    args=(--no-tui --output-format json -z "$DURATION" -c "$CONNECTIONS" -m "$method")
    [[ -n $body ]] && args+=(-T application/json -d "$body")
    [[ $name == sync && -n $TOKEN ]] && args+=(-H "Authorization: Bearer $TOKEN")

    oha "${args[@]}" "$URL$path" >"$RESULTS"
    jq -r --arg name "$name" '
        def ms: if . == null then "-" else (. * 1000 * 100 | round / 100 | tostring) + "ms" end;
        [ $name,
          (.summary.requestsPerSec | round | tostring),
          (.latencyPercentiles.p50 | ms),
          (.latencyPercentiles.p90 | ms),
          (.latencyPercentiles.p99 | ms),
          (.summary.slowest | ms),
          (.statusCodeDistribution | to_entries | map("\(.key)×\(.value)") | join(" "))
        ] | @tsv' "$RESULTS" \
        | awk -F'\t' '{ printf "%-20s %10s %9s %9s %9s %9s  %s\n", $1, $2, $3, $4, $5, $6, $7 }'
done
rm -f "$RESULTS"
