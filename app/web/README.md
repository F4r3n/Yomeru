# yomeru-web

The Yomeru website — a Dioxus 0.7 + WASM client that talks to
[`yomeru-server`](../../server/README.md) over HTTP.

All UI lives in [`yomeru-shared`](../shared); this crate is just the web
entry point (`fn main() { dioxus::launch(yomeru_shared::App) }`) and the
target the Dioxus CLI bundles.

## Prerequisites

- Rust + `wasm32-unknown-unknown` target — `rustup target add wasm32-unknown-unknown`
- Dioxus CLI — `cargo install dioxus-cli`
- A running `yomeru-server` reachable at the same origin (see "Server"
  below)

## Dev

```bash
# run from the workspace root, not from app/web/
dx serve --package yomeru-web
```

Opens `http://localhost:8080` with hot reload. Every keystroke in the
Lookup tab hits `/api/lookup` on the server.

> **Why from the root?** This workspace uses `default-members` to keep
> the Android crate out of host builds. Dioxus CLI 0.7.9 canonicalizes
> each default-member path against the current directory, so running
> `dx serve` from `app/web/` panics with `Os { code: 2, kind: NotFound }`
> when it tries to resolve `"server"` as `app/web/server`. Run from the
> workspace root and pass `--package yomeru-web`.

## Production build

```bash
dx bundle --package yomeru-web --platform web --release
```

Output: `target/dx/yomeru-web/release/web/public/` — static files (HTML,
JS glue, hashed `.wasm`). Serve this directory from any static host
that can also reverse-proxy `/api/*` to `yomeru-server`.

The release `.wasm` is ~2.3 MB. No dict bytes are bundled — all lookup
goes to the server.

### Build via Docker

If you don't want Rust + `dioxus-cli` on the build host, build inside
Docker and extract the bundle. From the workspace root:

```bash
docker build -f app/web/Dockerfile --target export \
    --output type=local,dest=./web-dist .
```

`./web-dist/` then contains the same files as
`target/dx/yomeru-web/release/web/public/`.

## Server

The site is useless without `yomeru-server` running on the same origin
(or behind a reverse proxy that forwards `/api/*` to it). The server
holds the FST and entry blobs in memory; the website only knows about
cards, settings, and sync.

```bash
# in another terminal
cargo run -p server -- \
    --data-dir extension/data \
    --smtp-host smtp.example.com \
    --smtp-from yomeru@example.com
```

`extension/data/` must contain `jmdict.bin`, `kanjidic.bin`,
`examples.bin` — build them once with `cargo xtask build-all`.

For dev, the simplest setup is one reverse proxy fronting both
`dx serve` (HTML/JS/wasm) and the server (`/api/*`). Caddy or nginx
works; example Caddyfile:

```
:8000 {
    handle_path /api/* {
        reverse_proxy localhost:8080
    }
    handle {
        reverse_proxy localhost:8081  # dx serve
    }
}
```

(`dx serve --port 8081` to free up 8080 for the server.)

## What the website does

Six tabs ported from the Firefox extension:

| Tab        | What it does                                                            |
|------------|-------------------------------------------------------------------------|
| Review     | FSRS-scheduled review session — front/back, 4-button rating, kanji + examples on the back |
| New Words  | Staging area: accept/reject newly-added words before they enter review  |
| Word List  | All active cards, filterable, with state/due-time badges                |
| Lookup     | Manual dictionary lookup with romaji-to-hiragana input + recent history |
| Settings   | FSRS knobs, JSON backup/restore, OTP-based server sync                  |
| About      | Data sources + scheduler info                                           |

The hover-to-look-up content script and on-page SRS highlighter from
the extension are intentionally **not** here — the website has no host
page to read.

## Storage

- Cards → IndexedDB (`yomeru-db`, store `cards`, v4 schema — identical
  shape to the extension, so backup JSON moves freely between them)
- Settings → `localStorage` key `srs_settings`
- Lookup history → `localStorage` key `lookup_history`
- Session token (after OTP verify) → inside the settings blob
