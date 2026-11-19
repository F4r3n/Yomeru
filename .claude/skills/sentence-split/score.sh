#!/bin/sh
# Corpus scores of the segmenter and the grammar breakdown.
# Usage: score.sh TAG [SEG_COSTS]  → reports in ~/.cache/yomeru-sentence/{seg,gram}-TAG.tsv
set -e
tag=${1:?usage: score.sh TAG [costs]}
c=$HOME/.cache/yomeru-sentence
cd "$(dirname "$0")/../../.."
[ -f "$c/jmdict-full.bin" ] || { echo "no $c/jmdict-full.bin: run setup.sh"; exit 1; }
JMDICT_BIN=$c/jmdict-full.bin SEG_COSTS=${2:-} SEG_REPORT=$c/seg-$tag.tsv \
  cargo test -q --release -p jmdict-core --test segmentation segmentation_against_the_corpus -- --ignored --nocapture 2>&1 \
  | grep -iE "f1|exact|boundar|panicked|^error" | head -6
JMDICT_BIN=$c/jmdict-full.bin GRAMMAR_REPORT=$c/gram-$tag.tsv \
  cargo test -q --release -p sentence-core --test grammar_breakdown grammar_found -- --ignored --nocapture 2>&1 \
  | grep -E "^all|other matches per|panicked|^error"
