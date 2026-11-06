#!/usr/bin/env bash
# Build the Yomeru Android APK inside the yomeru-android-build image.
# Extra args go to `dx bundle` (e.g. --release).
# Set YOMERU_ANDROID_SHELL=1 to get a shell in the container instead.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
image=yomeru-android-build

# The offline dictionaries, bundled as assets (see src/main.rs). Hard links,
# so the ~70 MB isn't stored twice; `asset!` rejects symlinks that leave the
# crate. Produced by `cargo xtask build-all`.
dict_src="$root/extension/data"
dict_dst="$root/app/android/assets/dict"
mkdir -p "$dict_dst"
for f in jmdict.bin kanjidic.bin examples.bin kanji-mnemonics.tsv part-names.tsv; do
    if [[ ! -f "$dict_src/$f" ]]; then
        echo "missing $dict_src/$f — run: cargo xtask build-all" >&2
        exit 1
    fi
    ln -f "$dict_src/$f" "$dict_dst/$f" 2>/dev/null || cp -f "$dict_src/$f" "$dict_dst/$f"
done

if ! podman image exists "$image"; then
    podman build -f "$root/app/android/Dockerfile" -t "$image" "$root/app/android"
fi

# Phones are arm64; dx would otherwise default to the host arch (x86_64),
# which only runs on the emulator. Pass your own --target to override.
target=(--target aarch64-linux-android)
for arg in "$@"; do
    [[ "$arg" == --target* ]] && target=()
done

cmd=(dx bundle --package yomeru-android --platform android "${target[@]}" "$@")
tty=()
if [[ "${YOMERU_ANDROID_SHELL:-}" == 1 ]]; then
    cmd=(bash)
    tty=(-it)
fi

exec podman run --rm "${tty[@]}" \
    -e YOMERU_SERVER_URL \
    -v "$root":/src:Z \
    -v yomeru-cargo-registry:/usr/local/cargo/registry \
    -v yomeru-gradle:/root/.gradle \
    -v yomeru-android-keys:/root/.android \
    "$image" "${cmd[@]}"
