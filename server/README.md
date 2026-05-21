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
| `YOMERU_DATA_DIR`  | `./data`           | Dir holding `jmdict.bin` / `kanjidic.bin` / `examples.bin`. **Required at startup.** Build with `cargo xtask build-all`. |
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

This builds the image, mounts a `yomeru-data` volume at `/data` (SQLite persists across restarts), and loads settings from `.env`.

Logs / lifecycle:
```bash
docker compose logs -f
docker compose down          # stop, keep the volume
docker compose down -v       # stop and wipe the SQLite volume
```

### Build / run without compose

```bash
# from the workspace root
docker build -f server/Dockerfile -t yomeru-server .

docker run -d --name yomeru-server \
  --env-file server/.env \
  -p 8080:8080 \
  -v yomeru-data:/data \
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
