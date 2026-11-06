#!/usr/bin/env bash
# Turn the release .aab into a signed universal APK.
#
#   sign-release.sh <app.aab> <keystore.jks> <key-alias> <out.apk>
#
# Needs: java, $ANDROID_HOME (build-tools), $BUNDLETOOL (path to the
# bundletool jar) and the keystore password in $YOMERU_KS_PASS.
#
# Shared by both build paths so they sign identically: podman-build.sh runs
# it inside the toolchain image, the release workflow directly on the runner.
#
# Why the .aab: dx's APK uses Gradle's debug build type (debuggable) even
# with --release, while the .aab is the real release build (minified, not
# debuggable). bundletool builds a universal APK from it and signs it.
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "usage: $0 <app.aab> <keystore.jks> <key-alias> <out.apk>" >&2
    exit 2
fi
aab=$1 keystore=$2 alias=$3 out=$4
: "${YOMERU_KS_PASS:?set YOMERU_KS_PASS to the keystore password}"
: "${ANDROID_HOME:?ANDROID_HOME must point at the Android SDK}"
: "${BUNDLETOOL:?BUNDLETOOL must point at the bundletool jar}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# bundletool's --ks-pass takes no env var: pass the password through a
# private file in a temp dir removed on exit.
(umask 077; printf %s "$YOMERU_KS_PASS" > "$tmp/ks-pass")
java -jar "$BUNDLETOOL" build-apks --mode=universal --overwrite \
    --bundle="$aab" --output="$tmp/yomeru.apks" \
    --ks="$keystore" --ks-key-alias="$alias" \
    --ks-pass="file:$tmp/ks-pass" --key-pass="file:$tmp/ks-pass"

mkdir -p "$(dirname "$out")"
unzip -p "$tmp/yomeru.apks" universal.apk > "$out"

tools="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
# 16 KB page alignment for Android 15+ (see .cargo/config.toml).
"$tools/zipalign" -c -P 16 4 "$out"
"$tools/apksigner" verify --print-certs "$out" | head -2
