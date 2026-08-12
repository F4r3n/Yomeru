# Deploying yomeru-web behind nginx

End-to-end guide for putting the website on a public box: static files
served by nginx, dict + sync API proxied to `yomeru-server` running on
localhost.

Assumptions: Ubuntu/Debian-flavored Linux, a real DNS name, you've
already pointed it at the box. Substitute `yomeru.example.com`
throughout.

---

## 1. Build the dict bins (once, on a build host)

The server needs three binary files at startup
(`jmdict.bin` / `kanjidic.bin` / `examples.bin`).

**Podman deploy** (compose, below): you can skip this section — the
`yomeru-dicts` builder service in `server/docker-compose.yml`
produces the bins inside the cluster and writes them to a named
volume. The server image itself doesn't bake them in. Jump to
section 2.

**systemd deploy** (Option B in section 3): build them on any host
with the Rust toolchain — the bins are deterministic and portable.

```bash
cargo xtask download-dict
cargo xtask download-kanjidic
cargo xtask download-examples
cargo xtask build-all
# ↳ writes extension/data/{jmdict,kanjidic,examples}.bin
```

Total ≈45 MB. Copy that directory to the deploy host (e.g.
`/srv/yomeru/dicts/`).

## 2. Build the website bundle

On a host with Rust + `dx`:

```bash
cargo install dioxus-cli   # if not already installed
dx bundle --package yomeru-web --platform web --release
```

Output: `target/dx/yomeru-web/release/web/public/` — pure static files.
Copy that directory to the deploy host (e.g. `/srv/yomeru/web/`).

Or, if you only have Podman on the build host (Docker works too — the
Dockerfile is runtime-agnostic), run the bundle inside a container and
extract the artifacts (workspace root as context):

```bash
podman build -f app/web/Dockerfile --target export \
    --output type=local,dest=./web-dist .
```

`./web-dist/` is the same tree as
`target/dx/yomeru-web/release/web/public/`.

The release `.wasm` is ~2 MB; first-load assets are well under 5 MB
total. No dict bytes are bundled — every lookup is an HTTP call.

## 3. Build & run the server

Either with Podman (simplest) or as a systemd unit. Both work; pick
one.

### Option A: Podman

```bash
# on the deploy host, in a checkout of this repo
cd server
cp .env.example .env
# edit .env — at minimum YOMERU_SMTP_HOST and YOMERU_SMTP_FROM,
# plus YOMERU_SMTP_USER/PASS if your relay requires auth
```

The compose file defines two services that share named volumes:

- `yomeru-dicts` — one-shot builder. Runs `cargo xtask build-all`
  inside its image, writes `jmdict.bin` / `kanjidic.bin` /
  `examples.bin` to the `yomeru-dicts` named volume, then exits 0.
  Skips itself on subsequent runs if the volume is already
  populated.
- `yomeru-server` — long-running. `depends_on` `yomeru-dicts` with
  `service_completed_successfully`, so first-time `up` automatically
  builds the dicts before the server starts. Mounts the
  `yomeru-dicts` volume read-only at
  `/usr/share/yomeru-server/dicts` (the image's default
  `YOMERU_DATA_DIR`), and the independent `yomeru-data` volume at
  `/data` for SQLite.

```bash
podman-compose up -d --build
```

First run takes ~5–10 min while the dict builder downloads JMdict /
KANJIDIC / examples and indexes them. Subsequent `up`s are
instant — the builder sees the volume populated and exits straight
away.

The server is now listening on `127.0.0.1:8080` (publish only to
localhost — nginx fronts it).

To bind only to localhost, edit the `ports:` line in
`server/docker-compose.yml`:

```yaml
ports:
  - "127.0.0.1:8080:8080"
```

**Refreshing the dictionaries.** Because the bins live on their own
volume rather than in the image, you can refresh them without
rebuilding the server or touching SQLite:

```bash
podman-compose run --rm -e FORCE=1 yomeru-dicts   # rebuild from scratch
podman-compose restart yomeru-server              # server caches dicts in memory
```

`yomeru-data` (SQLite) is untouched.

### Option B: systemd

Build the binary on the deploy host:

```bash
cargo build --release -p server
sudo install -m 0755 target/release/yomeru-server /usr/local/bin/
```

```ini
# /etc/systemd/system/yomeru-server.service
[Unit]
Description=Yomeru sync + dict-lookup server
After=network.target

[Service]
Type=simple
User=yomeru
Group=yomeru
WorkingDirectory=/srv/yomeru
Environment=YOMERU_PORT=8080
Environment=YOMERU_DB_PATH=/srv/yomeru/yomeru.db
Environment=YOMERU_DATA_DIR=/srv/yomeru/dicts
Environment=YOMERU_SMTP_HOST=smtp.example.com
Environment=YOMERU_SMTP_PORT=587
Environment=YOMERU_SMTP_FROM=noreply@example.com
# Rate limiting keys on the client IP. Behind nginx the peer address is
# always loopback, so the server reads X-Real-IP / X-Forwarded-For — but
# only from a trusted peer. See "Client IP & rate limiting" below.
Environment=YOMERU_TRUST_PROXY=private
Environment=YOMERU_TRUSTED_PROXY_HOPS=1
EnvironmentFile=-/etc/yomeru/secrets.env
ExecStart=/usr/local/bin/yomeru-server
Restart=on-failure
RestartSec=5s

# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/srv/yomeru

[Install]
WantedBy=multi-user.target
```

Put `YOMERU_SMTP_USER` / `YOMERU_SMTP_PASS` in
`/etc/yomeru/secrets.env` (mode `0600`, owner `root`) — keep them out
of the unit file.

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now yomeru-server
sudo systemctl status yomeru-server
```

## 4. nginx

The nginx config does three things: serves the static bundle, proxies
`/api/*` to the server, and applies the right caching headers
(`.wasm` is fingerprinted so it's safe to cache forever).

```nginx
# /etc/nginx/sites-available/yomeru
server {
    listen 80;
    listen [::]:80;
    server_name yomeru.example.com;
    # Let Certbot answer the ACME challenge, then 301 everything else.
    location /.well-known/acme-challenge/ {
        root /var/www/certbot;
    }
    location / { return 301 https://$host$request_uri; }
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name yomeru.example.com;

    ssl_certificate     /etc/letsencrypt/live/yomeru.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/yomeru.example.com/privkey.pem;
    include             /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam         /etc/letsencrypt/ssl-dhparams.pem;

    # Static bundle from `dx bundle`.
    root /srv/yomeru/web;
    index index.html;

    # Hashed assets (manganis fingerprints filenames) — cache forever.
    location /assets/ {
        access_log off;
        add_header Cache-Control "public, max-age=31536000, immutable";
        try_files $uri =404;
    }

    # The wasm + JS glue dx emits at the root. Long cache plus a fingerprint
    # in the filename keeps the browser from re-downloading the heavy bits.
    location ~* \.(?:wasm|js|css)$ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        gzip_static on;
        brotli_static on;     # only if nginx-brotli is installed
        try_files $uri =404;
    }

    # API → yomeru-server. No buffering so OTP-send latency reflects reality.
    location /api/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        # Both headers above are required — without them every request looks
        # like it came from 127.0.0.1 and the per-IP rate limits collapse
        # into a single shared bucket for the whole internet.
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
        client_max_body_size 4m;          # /api/sync can carry a few hundred cards
    }

    # SPA fallback — the router lives client-side, so unknown paths still
    # render index.html and let Dioxus pick the route.
    location / {
        try_files $uri $uri/ /index.html;
    }

    # Don't ship JSON to anyone snooping.
    add_header X-Content-Type-Options nosniff;
    add_header Referrer-Policy strict-origin-when-cross-origin;

    # Compress the .wasm on the fly if `gzip_static` isn't available.
    gzip on;
    gzip_types application/wasm application/javascript application/json text/css;
}
```

Enable + reload:

```bash
sudo ln -s /etc/nginx/sites-available/yomeru /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

TLS via Certbot (one-time):

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d yomeru.example.com
```

### Client IP & rate limiting

The auth endpoints (10/min) and lookup endpoints (20/s) are rate-limited
per client IP. Behind a proxy the TCP peer is the proxy, not the client,
so the server falls back to forwarding headers — but only when the peer
is itself trusted, otherwise anyone could spoof their way out of their
own bucket.

| Variable | Default | Meaning |
|---|---|---|
| `YOMERU_TRUST_PROXY` | `private` | `none` = always use the peer address (use when the server is exposed directly, with no proxy). `private` = trust loopback / RFC1918 / unique-local / link-local peers; covers nginx-on-localhost and container bridges. `all` = trust every peer; only safe if something upstream always overwrites the headers. |
| `YOMERU_TRUSTED_PROXY_HOPS` | `1` | How many trusted proxies sit in front. `X-Forwarded-For` is append-only, so the value is counted this many entries in from the right; anything further left is client-supplied and ignored. Set to `2` for e.g. Cloudflare → nginx. |

Getting `HOPS` wrong is not just cosmetic: too high and you rate-limit on
an attacker-controlled value, too low and you rate-limit on your own
proxy. Check it against the logs after any change to the proxy chain.

## 5. Smoke test

```bash
# Server reachable on loopback?
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/lookup \
    -H 'Content-Type: application/json' -d '{"words":["飲む"]}'
# → 200

# Through nginx?
curl -s -o /dev/null -w '%{http_code}\n' https://yomeru.example.com/api/lookup \
    -H 'Content-Type: application/json' -d '{"words":["飲む"]}'
# → 200

# Static bundle?
curl -sI https://yomeru.example.com/ | head -3
# → 200 OK, content-type: text/html
```

Open `https://yomeru.example.com/` in a browser, go to the Lookup tab,
type `飲む` (or `nomu` — romaji works). You should see entries.

## Updating

### Before updating: back up cards

Any release that changes the server's `cards` table shape (see
CHANGELOG/commit history for "clean break" schema changes — e.g. the
word→sequence rekey, and later the `priority` column) drops and recreates
that table on the next `yomeru-server` restart: the sync merge point is
wiped, not migrated. Local IndexedDB on each device is never touched and
stays the source of truth, but do this first so no device is caught with
unsynced data:

1. On every device that uses the site/extension: Settings → Export, save
   the JSON somewhere safe.
2. Update, using the commands below (or run `./deploy.sh` from the repo
   root, which does both the website rebuild and
   `podman-compose -f server/docker-compose.yml up -d --build` for the
   server in one shot).
3. Open the site/extension on each device again — auto-sync (or Settings
   → Sync now) repopulates the fresh server table from local IndexedDB.
4. Only if a device's local IndexedDB was itself lost: Settings → Import
   the JSON from step 1, then sync.

```bash
# new server binary (Podman — only rebuilds the server stage, dict volume untouched)
podman-compose -f server/docker-compose.yml up -d --build yomeru-server
# or, for systemd:
cargo build --release -p server && sudo install -m 0755 target/release/yomeru-server /usr/local/bin/ && sudo systemctl restart yomeru-server

# refreshed dict bins (Podman)
podman-compose -f server/docker-compose.yml run --rm -e FORCE=1 yomeru-dicts
podman-compose -f server/docker-compose.yml restart yomeru-server

# new website bundle
dx bundle --package yomeru-web --platform web --release
rsync -av --delete target/dx/yomeru-web/release/web/public/ deploy-host:/srv/yomeru/web/
```

The hashed asset filenames mean browsers will fetch the new `.wasm`
the first time someone hits the site after the rsync — no cache
busting needed.

## Layout on the deploy host

```
/srv/yomeru/
├── web/                # static bundle from `dx bundle`
│   ├── index.html
│   ├── assets/...
│   └── yomeru-web_bg-<hash>.wasm
├── dicts/              # jmdict.bin, kanjidic.bin, examples.bin
└── yomeru.db           # SQLite — sync state
```

## Troubleshooting

- **Server exits immediately with `failed to load dict data from …`**
  → `YOMERU_DATA_DIR` is wrong or one of the three `.bin` files is
  missing. Verify with `ls "$YOMERU_DATA_DIR"`.
- **`/api/lookup` returns 404 via nginx but 200 on `127.0.0.1:8080`**
  → check the `proxy_pass` line. No trailing slash on `/api/` →
  forwarded as `/api/lookup`. With a trailing slash on the target
  (`proxy_pass http://127.0.0.1:8080/;`) nginx strips the `/api/`
  prefix, which is wrong here.
- **`POST /api/sync` returns 413** → bump `client_max_body_size` in
  the nginx server block; the default 1 MB is enough for ~3 k cards
  but tight if someone imports a big backup.
- **`.wasm` served as `application/octet-stream`** → nginx's mime
  types file should include `application/wasm wasm;`. On Debian it's
  `/etc/nginx/mime.types`; add the line or upgrade nginx.
