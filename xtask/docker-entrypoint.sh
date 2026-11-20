#!/bin/sh
# Entrypoint for the yomeru-dicts builder image. See xtask/Dockerfile.
#
# Behaviour:
#   - Compile data/grammar.toml into /dicts/grammar.bin (always: it comes
#     with the image).
#   - If /dicts already holds the three other .bin files, in the format
#     versions this image's readers take, and FORCE != 1, exit
#     immediately. Lets `depends_on` keep `docker compose up` cheap
#     after the first run.
#   - Otherwise: run `cargo xtask build-all` (downloads JMdict_e,
#     kanjidic2, examples.utf if not present in /build), then copy the
#     fresh .bin files into /dicts so the server volume sees them.
set -eu

mkdir -p /dicts /build/extension/data

# The overlay files (mnemonics, part names) ship in the image, not the dict
# build, so a new image updates them even when the dictionaries are kept.
for f in kanji-mnemonics.tsv part-names.tsv; do
    if [ -f "/build/data/$f" ]; then
        cp "/build/data/$f" "/dicts/$f"
    fi
done

# Same for the grammar dictionary: compiled from the image's
# data/grammar.toml on every start, in about a second.
cd /build
cargo run --release -p xtask -- build-grammar --output /dicts/grammar.bin

# The format version a reader takes, from its `if version != N {` line,
# and the one a .bin holds (the byte after its 4-byte magic). A new image
# whose reader takes another version rebuilds: an old jmdict.bin left in
# the volume would stop the server from starting.
wanted() { sed -n 's/.*if version != \([0-9]*\) {.*/\1/p' "$1" | head -n 1; }
held() { od -An -tu1 -j4 -N1 "$1" 2>/dev/null | tr -d ' '; }
current=1
for pair in jmdict.bin:crates/jmdict-core/src/dictionary.rs \
    examples.bin:crates/examples-core/src/lib.rs; do
    bin=/dicts/${pair%%:*}
    want=$(wanted "${pair#*:}")
    if [ -s "$bin" ] && [ -n "$want" ] && [ "$(held "$bin")" != "$want" ]; then
        echo "yomeru-dicts: $bin is format $(held "$bin"), this image reads $want; rebuilding."
        current=0
    fi
done

if [ "${FORCE:-0}" != "1" ] \
    && [ "$current" = 1 ] \
    && [ -s /dicts/jmdict.bin ] \
    && [ -s /dicts/kanjidic.bin ] \
    && [ -s /dicts/examples.bin ]; then
    echo "yomeru-dicts: /dicts already populated; skipping build. Set FORCE=1 to rebuild."
    exit 0
fi

cd /build
# build-dicts (not build-all) — we don't have wasm-pack or npm in
# this image, and the server only needs the three .bin files.
cargo run --release -p xtask -- build-dicts

cp extension/data/jmdict.bin   /dicts/jmdict.bin
cp extension/data/kanjidic.bin /dicts/kanjidic.bin
cp extension/data/examples.bin /dicts/examples.bin

echo "yomeru-dicts: wrote jmdict.bin / kanjidic.bin / examples.bin to /dicts."
