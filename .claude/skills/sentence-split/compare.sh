#!/bin/bash
# What a change moved: the corpus words newly wrong (>) and newly right (<)
# between two score.sh runs, grouped and counted.
# Usage: compare.sh OLD_TAG NEW_TAG [N]
c=$HOME/.cache/yomeru-sentence
n=${3:-15}
echo "== segmentation (count, ours, corpus)"
diff "$c/seg-$1.tsv" "$c/seg-$2.tsv" | grep "^[<>]" | cut -f1,2 | sort | uniq -c | sort -rn | head -"$n"
echo "== grammar examples"
diff <(cut -f1,2 "$c/gram-$1.tsv") <(cut -f1,2 "$c/gram-$2.tsv") | grep "^[<>]" | cut -c1-220 | head -"$n"
