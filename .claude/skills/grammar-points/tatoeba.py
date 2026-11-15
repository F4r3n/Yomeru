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


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("regex")
    ap.add_argument("--count", type=int, default=10)
    ap.add_argument("--min", type=int, default=8, help="shortest sentence, in chars")
    ap.add_argument("--max", type=int, default=26, help="longest sentence, in chars")
    ap.add_argument("--file", default=str(EXAMPLES))
    args = ap.parse_args()

    pattern = re.compile(args.regex)
    hits = []
    with open(args.file, encoding="utf-8") as f:
        for line in f:
            if not line.startswith("A: "):
                continue
            ja, _, rest = line[3:].rstrip("\n").partition("\t")
            en, _, sid = rest.partition("#ID=")
            if pattern.search(ja) and args.min <= len(ja) <= args.max:
                hits.append((len(ja), ja, en, sid))
    hits.sort()
    step = max(1, len(hits) // args.count)
    print(f"{len(hits)} matches")
    for _, ja, en, sid in hits[::step][: args.count]:
        print(f"{sid} {ja} | {en}")


if __name__ == "__main__":
    main()
