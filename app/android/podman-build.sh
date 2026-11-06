#!/usr/bin/env bash
# Build the Yomeru Android APK inside the yomeru-android-build image.
# Extra args go to `dx bundle`.
#
#   app/android/podman-build.sh             debug APK, signed with a debug key
#   app/android/podman-build.sh --release   release APK, signed with the
#                                           release key → dist/android/
#
# Release signing reads the keystore at $YOMERU_KEYSTORE (default
# ~/yomeru-release.jks, alias $YOMERU_KEY_ALIAS, default "yomeru") and asks
# for its password unless $YOMERU_KS_PASS is set. The keystore is mounted
# read-only and the password only ever lives in the container's environment.
#
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
release=0
for arg in "$@"; do
    [[ "$arg" == --target* ]] && target=()
    [[ "$arg" == --release || "$arg" == -r ]] && release=1
done

run=(podman run --rm
    -e YOMERU_SERVER_URL
    -v "$root":/src:Z
    -v yomeru-cargo-registry:/usr/local/cargo/registry
    -v yomeru-gradle:/root/.gradle
    -v yomeru-android-keys:/root/.android)

if [[ "${YOMERU_ANDROID_SHELL:-}" == 1 ]]; then
    exec "${run[@]}" -it "$image" bash
fi

if [[ $release == 1 ]]; then
    keystore="${YOMERU_KEYSTORE:-$HOME/yomeru-release.jks}"
    alias="${YOMERU_KEY_ALIAS:-yomeru}"
    if [[ ! -f "$keystore" ]]; then
        echo "release keystore not found: $keystore (set YOMERU_KEYSTORE)" >&2
        exit 1
    fi
    if [[ -z "${YOMERU_KS_PASS:-}" ]]; then
        read -rsp "Password for $keystore: " YOMERU_KS_PASS
        echo
    fi
    export YOMERU_KS_PASS
fi

"${run[@]}" "$image" \
    dx bundle --package yomeru-android --platform android "${target[@]}" "$@"

[[ $release == 1 ]] || exit 0

# ── Sign the release APK ─────────────────────────────────────────────
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/app/android/Cargo.toml" | head -1)"
out="yomeru-$version.apk"

# Same signing script the release workflow runs (see sign-release.sh).
aab="$(find "$root/target/podman/dx/yomeru/release/android" -name "*.aab" \
       -path "*outputs/bundle/release/*" | head -1)"
if [[ -z "$aab" ]]; then
    echo "no release .aab found under target/podman/dx/yomeru/release" >&2
    exit 1
fi
"${run[@]}" \
    -e YOMERU_KS_PASS \
    -v "$keystore":/keys/release.jks:ro,Z \
    "$image" /src/app/android/sign-release.sh \
        "/src/${aab#"$root"/}" /keys/release.jks "$alias" "/src/dist/android/$out"
echo "Signed: dist/android/$out"
