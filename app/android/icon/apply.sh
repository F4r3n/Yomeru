#!/usr/bin/env bash
# Put the Yomeru launcher icon and window theme (res/values*/styles.xml: a
# dark background from the first frame, no white flash on a cold start) into
# the Gradle project dx generated, then re-run the given Gradle task(s) so the
# APK / .aab carries them.
#
#   apply.sh <gradle-project-dir> <gradle-task>...
#   e.g. apply.sh target/dx/yomeru/release/android/app bundleRelease
#
# Why a second Gradle pass: dx 0.7 writes its own default icons and styles.xml into res/ on
# every build (embedded in the CLI, no config for it), so ours can only go in
# after `dx bundle`. The Rust library is already in jniLibs/, so this pass
# only repackages: seconds, not a rebuild.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <gradle-project-dir> <gradle-task>..." >&2
    exit 2
fi
project="$1"
shift
here="$(cd "$(dirname "$0")" && pwd)"
res="$project/app/src/main/res"

if [[ ! -x "$project/gradlew" || ! -d "$res" ]]; then
    echo "not a dx Android project: $project" >&2
    exit 1
fi

# dx ships its icons as .webp; ours are .png under the same resource name,
# and two files for one resource fail the build.
rm -f "$res"/mipmap-*/ic_launcher.webp
cp -r "$here/res/." "$res/"

# dx copies its bundle to outputs/bundle/<type>/Yomeru-<target>.aab, while
# Gradle writes app-<type>.aab next to it: drop dx's, or whoever picks "the"
# .aab may sign the one with dx's icons (0.24.0 did).
outputs="$project/app/build/outputs"
rm -f "$outputs"/bundle/*/*.aab

stamp="$(mktemp)"
trap 'rm -f "$stamp"' EXIT
(cd "$project" && ./gradlew --quiet "$@")

# Every package this pass wrote must carry our icon, not dx's. (Older ones
# stay, such as the debug APK of a release `dx bundle`: not ours to check.)
found=0
while IFS= read -r pkg; do
    found=1
    list="$(unzip -l "$pkg")"
    if ! grep -q 'res/drawable/ic_launcher_monochrome\.xml' <<<"$list" \
        || grep -q 'ic_launcher\.webp' <<<"$list"; then
        echo "$pkg still has dx's default launcher icon" >&2
        exit 1
    fi
done < <(find "$outputs" \( -name '*.aab' -o -name '*.apk' \) -newer "$stamp")
if [[ $found == 0 ]]; then
    echo "no .aab or .apk under $outputs after: gradlew $*" >&2
    exit 1
fi
