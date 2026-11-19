#!/bin/sh
# The breakdown (words + grammar points) of every sentence of a text file.
# Usage: split.sh FILE   (sentences end at 。！？ or a newline)
set -e
f=${1:?usage: split.sh FILE}
c=$HOME/.cache/yomeru-sentence
f=$(realpath "$f")
cd "$(dirname "$0")/../../.."
[ -f "$c/jmdict-full.bin" ] || { echo "no $c/jmdict-full.bin: run setup.sh"; exit 1; }
# One sentence per item; BREAKDOWN splits on ASCII commas.
list=$(python3 -I -c '
import re, sys
t = open(sys.argv[1], encoding="utf-8").read().replace(",", "、")
parts = re.findall(r"[^。！？!?\n]+[。！？!?]?", t)
print(",".join(p.strip() for p in parts if p.strip()))' "$f")
JMDICT_BIN=$c/jmdict-full.bin BREAKDOWN="$list" \
  cargo test -q --release -p sentence-core --test grammar_breakdown breakdown_of -- --ignored --nocapture 2>&1 \
  | grep -vE "^running|^test |^$|test result"
