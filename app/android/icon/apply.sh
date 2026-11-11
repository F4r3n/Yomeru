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

(cd "$project" && ./gradlew --quiet "$@")
