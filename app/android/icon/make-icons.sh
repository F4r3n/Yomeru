#!/usr/bin/env bash
# Regenerate the legacy launcher PNGs (Android 7.x, before adaptive icons)
# from the app logo. The adaptive layers in res/drawable*/ are hand-kept
# vector copies of the same glyph; update them too if icon.svg changes.
# Needs rsvg-convert (librsvg).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
svg="$here/../../shared/assets/icon.svg"
for pair in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
    density=${pair%%:*} px=${pair##*:}
    mkdir -p "$here/res/mipmap-$density"
    rsvg-convert -w "$px" -h "$px" "$svg" -o "$here/res/mipmap-$density/ic_launcher.png"
done
