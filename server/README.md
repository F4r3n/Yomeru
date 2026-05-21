# yomeru-server

Small Axum + SQLite service that backs the Yomeru extension and website:

- Email + OTP login (`/api/auth/request`, `/api/auth/verify`)
- Card sync (`/api/sync`) — merges incoming cards into a SQLite store, returning the merged set
- Dict lookup (`/api/lookup`, `/api/lookup-prefix`, `/api/kanji`, `/api/examples`) — reads
  JMdict / KANJIDIC / Tatoeba blobs into memory at startup; no auth required

OTPs are sent over SMTP. Sessions are 30-day bearer tokens. Per-IP rate limiting is applied to every endpoint.

## Configuration

All settings live in a single env file. Copy the example and fill it in:

```bash
cp .env.example .env
```

| Variable           | Default            | Notes                                                    |
|--------------------|--------------------|----------------------------------------------------------|
| `YOMERU_PORT`      | `8080`             | HTTP port the server listens on.                         |
| `YOMERU_DB_PATH`   | `/data/yomeru.db`  | SQLite file path. `/data` is the mounted volume.         |
| `YOMERU_DATA_DIR`  | `/usr/share/yomeru-server/dicts` (in Docker) / `./data` (host) | Dir holding `jmdict.bin` / `kanjidic.bin` / `examples.bin`. **Required at startup.** In compose this is the `yomeru-dicts` volume populated by the `yomeru-dicts` builder service; on the host, build with `cargo xtask build-all`. |
| `YOMERU_SMTP_HOST` | —                  | **Required.** STARTTLS relay host.                       |
| `YOMERU_SMTP_PORT` | `587`              |                                                          |
| `YOMERU_SMTP_FROM` | —                  | **Required.** Address OTP emails are sent from.          |
| `YOMERU_SMTP_USER` | —                  | Optional. Blank → unauthenticated relay.                 |
| `YOMERU_SMTP_PASS` | —                  | Optional.                                                |

Each setting also accepts an equivalent CLI flag (`--port`, `--db`, `--smtp-host`, …), which takes priority over the env var when both are set.

## Run with Docker

The Dockerfile lives in this directory but its build context is the workspace root (cargo needs the workspace `Cargo.toml`).

```bash
# from server/
docker compose up -d --build
```

Compose orchestrates two services that share three named volumes:

| Service        | Lifetime              | Volumes                                                                  |
|----------------|-----------------------|--------------------------------------------------------------------------|
| `yomeru-dicts` | one-shot (exits 0)    | writes `yomeru-dicts` (the three `.bin` files)                           |
| `yomeru-server`| long-running          | `yomeru-data` (SQLite) rw, `yomeru-dicts` ro at `/usr/share/yomeru-server/dicts` |

`yomeru-server` `depends_on` `yomeru-dicts` with `service_completed_successfully`, so first-time `up` automatically builds the dicts before the server starts. On subsequent `up`s the builder sees the volume populated, exits immediately, and the server starts straight away.

### Refresh the dictionaries

Because the dict bins live on their own volume — not in the image and not bundled with SQLite — you can refresh them without rebuilding the server or touching user data:

```bash
docker compose run --rm -e FORCE=1 yomeru-dicts   # re-download + re-build
docker compose restart yomeru-server              # server caches dicts in memory
```

`yomeru-data` (SQLite) is untouched. `FORCE=1` is what bypasses the "already populated" check in the builder entrypoint.

Logs / lifecycle:
```bash
docker compose logs -f
docker compose down          # stop, keep the volumes
docker compose down -v       # stop and wipe BOTH volumes (SQLite + dicts)
```

### Build / run without compose

```bash
# from the workspace root — server image
docker build -f server/Dockerfile -t yomeru-server .

# populate the dict volume once
docker volume create yomeru-dicts
docker build -f xtask/Dockerfile -t yomeru-dicts .
docker run --rm -v yomeru-dicts:/dicts yomeru-dicts

docker run -d --name yomeru-server \
  --env-file server/.env \
  -p 8080:8080 \
  -v yomeru-data:/data \
  -v yomeru-dicts:/usr/share/yomeru-server/dicts:ro \
  yomeru-server
```

## Run without Docker

```bash
cargo run -p server --release -- \
  --smtp-host smtp.example.com \
  --smtp-from noreply@example.com \
  --smtp-user '...' --smtp-pass '...'
```

Or export the same values as `YOMERU_*` env vars and just run `cargo run -p server --release`.
