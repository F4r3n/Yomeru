# Yomeru app (Dioxus)

Cross-platform Yomeru client built with Dioxus.

```
app/
├── shared/   # Shared UI + storage + dict glue (yomeru-shared)
├── web/      # Web entry point (yomeru-web)
└── android/  # Android entry point (yomeru-android)
```

Tabs ported from the extension: Review (FSRS), New Words, Word List,
Lookup, Settings, About. The hover-on-page detector and SRS highlighter
are intentionally omitted — there's no host page on a website.

## Dictionary lookup

Lookups go over HTTP to the yomeru-server on the same origin:
`/api/lookup` (batched), `/api/lookup-prefix`, `/api/kanji`,
`/api/examples`. No dict bytes are bundled with the website.

For local dev, run the server alongside `dx serve` and point the
website at it via a proxy in `Dioxus.toml` (or run both on the same
host:port). See `server/README.md` for server setup; `server/data/`
must contain `jmdict.bin`, `kanjidic.bin`, `examples.bin` (build with
`cargo xtask build-all`).

## Prerequisites

Install the Dioxus CLI:

```bash
cargo install dioxus-cli
```

## Web

```bash
cd app/web
dx serve              # dev server with hot reload
dx build --release    # production bundle in target/dx/yomeru-web
```

## Android

Requires the Android SDK + NDK and `cargo-ndk`. See the Dioxus mobile guide:
<https://dioxuslabs.com/learn/0.7/guides/mobile>.

```bash
cd app/android
dx serve --platform android    # run on connected device / emulator
dx bundle --platform android   # produce an APK
```
