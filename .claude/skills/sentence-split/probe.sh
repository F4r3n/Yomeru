#!/bin/sh
# The segmenter's debug probe, against the full dictionary.
# Usage: probe.sh [words=W1,W2] [at=文:3,文:4] [sentences=S1,S2] [costs=k=v,…]
#   words      dictionary entries spelled W, with frequency and classes
#   at         every candidate word starting at char N of 文, with its cost
#   sentences  the lattice's split, each word with its class
#   costs      other values for segment::Costs (Costs::with keys)
c=$HOME/.cache/yomeru-sentence
cd "$(dirname "$0")/../../.."
for a in "$@"; do
  case $a in
    words=*) export SEG_WORDS=${a#words=} ;;
    at=*) export SEG_AT=${a#at=} ;;
    sentences=*) export SEG_SENTENCES=${a#sentences=} ;;
    costs=*) export SEG_COSTS=${a#costs=} ;;
    *) echo "unknown argument $a"; exit 1 ;;
  esac
done
JMDICT_BIN=$c/jmdict-full.bin \
  cargo test -q --release -p jmdict-core --test segmentation seg_debug -- --ignored --nocapture 2>&1 \
  | grep -vE "^running|^test |^$|test result"
