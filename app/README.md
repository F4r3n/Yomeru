# Yomeru app (Dioxus)

Cross-platform Yomeru client built with Dioxus.

```
app/
├── core/     # App logic, no UI or Dioxus: storage, SRS, sync, dict clients (yomeru-core)
├── shared/   # Shared Dioxus UI on top of core (yomeru-shared)
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

### Upgrading dx (Dioxus CLI)

dx is pinned in `app/android/Dockerfile` (`DX_VERSION`) and
`.github/workflows/release.yml` (`DX_VERSION`), and must match the `dioxus`
version in `Cargo.lock`. Bump all three together.

`app/android/MainActivity.kt` and `app/android/AndroidManifest.xml` replace
the files dx generates (`android_main_activity` / `android_manifest` in
`app/android/Dioxus.toml`). Both are copies of **dx 0.7.10**'s templates plus
our changes, so template updates from dx no longer reach the app on their
own. On each dx bump:

1. Build once, then diff our files against what the new dx would generate.
   The templates live in the dioxus repo under
   `packages/cli/assets/android/` (`MainActivity.kt.hbs`,
   `gen/app/src/main/AndroidManifest.xml.hbs`) for the new tag.
2. Port any template changes (permissions, attributes, activity code) into
   our copies, keeping our additions:
   - `AndroidManifest.xml`: `android:windowSoftInputMode="adjustResize|stateUnchanged"`
     on the activity.
   - `MainActivity.kt`: `FocusShield` (keeps the keyboard open when its
     "Change keyboard" picker opens), the `YomeruShell` background bridge, and
     the saved window background (`onCreate`, `rememberBackground`), so a cold
     start paints the user's theme before the page loads.
3. Check that wry still attaches the WebView with
   `activity.setContentView(view)` (`wry/src/android/main_pipe.rs`). The
   shield is installed by overriding that method: if wry attaches it another
   way, the shield is silently skipped and the keyboard bug comes back.
4. Update the "dx 0.7.10" mentions in both files and in this section.
5. Test on a phone: long-press the keyboard's globe key in a text box — the
   picker must stay open — and open/close the keyboard in both themes with no
   white strip. Cold-start the app in both themes: no flash of the other
   theme's colour (or white).
