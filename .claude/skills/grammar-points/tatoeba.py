"""Lists Tatoeba sentences matching a pattern, as example candidates.

Reads examples.utf (the Tanaka Corpus file the examples dictionary is built
from: Tatoeba sentences, CC BY 2.0 FR). Each line printed is
`<id> <japanese> | <english>`; the id goes into the entry as `tatoeba`.

  python3 -I tatoeba.py 'ようにし(て|た|ま|な)' [--count 10] [--min 8] [--max 26]

The regex runs on the Japanese side. Short sentences come first, spread
over the length range so the pick isn't only fragments.
"""

import argparse
import re
from pathlib import Path

EXAMPLES = Path(__file__).resolve().parents[3] / "examples.utf"


def load(path=EXAMPLES):
    """Every corpus sentence, as (ja, en, id)."""
    out = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.startswith("A: "):
                continue
            ja, _, rest = line[3:].rstrip("\n").partition("\t")
            en, _, sid = rest.partition("#ID=")
            out.append((ja, en, sid))
    return out


def pick(sentences, regex, count=10, lo=8, hi=26):
    """(match count, up to `count` lines): short ones first, spread over the range."""
    pattern = re.compile(regex)
    hits = sorted((len(ja), ja, en, sid) for ja, en, sid in sentences
                  if lo <= len(ja) <= hi and pattern.search(ja))
    step = max(1, len(hits) // count) if count else 1
    return len(hits), [f"{sid} {ja} | {en}" for _, ja, en, sid in hits[::step][:count]]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("regex")
    ap.add_argument("--count", type=int, default=10)
    ap.add_argument("--min", type=int, default=8, help="shortest sentence, in chars")
    ap.add_argument("--max", type=int, default=26, help="longest sentence, in chars")
    ap.add_argument("--file", default=str(EXAMPLES))
    args = ap.parse_args()

    n, lines = pick(load(args.file), args.regex, args.count, args.min, args.max)
    print(f"{n} matches")
    for line in lines:
        print(line)


if __name__ == "__main__":
    main()
