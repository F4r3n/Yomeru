#!/usr/bin/env bash
# Build the Yomeru Android APK inside the yomeru-android-build image.
# Extra args go to `dx bundle` (e.g. --release).
# Set YOMERU_ANDROID_SHELL=1 to get a shell in the container instead.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
image=yomeru-android-build

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
