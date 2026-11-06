#!/usr/bin/env python3
"""List the next kanji that need a mnemonic phrase, one compact line each:

    慣 [accustomed; get used to; become experienced] 忄=heart/standing heart + 貫=pierce/penetrate

No API involved: this only prepares the input; Claude Code writes the
phrases in the session (see SKILL.md). Part meanings go through the same
rules as the app's chips (part names from
data/part-names.tsv first, KANJIDIC radical labels trimmed), so phrases use
the words the learner sees. Kanji already in data/kanji-mnemonics.tsv are
skipped.

    next_kanji.py ENTRIES.json [--count N] [--only 惑論] [--deck BACKUP.json]
    next_kanji.py ENTRIES.json --check     # validate the TSV, list nothing

ENTRIES.json comes from `kanjidic-build --dump-json`. --deck takes an app
backup export and puts the kanji cards in it first, soonest due first.
"""

import argparse
import collections
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DATA = ROOT / "data"
TSV = DATA / "kanji-mnemonics.tsv"


def read_tsv(path):
    """`<char><TAB><text>` lines, as `kanjidic_core` reads them."""
    out = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.lstrip().startswith("#") or "\t" not in line:
                continue
            k, text = line.split("\t", 1)
            if len(k.strip()) == 1 and text.strip():
                out[k.strip()] = text.strip()
    return out


# Plain part names ("roof" for 宀), shown first in the chips too.
PART_NAMES = read_tsv(DATA / "part-names.tsv")


def part_meanings(meanings):
    """Same rule as `kanjidic_types::part_meanings`: trim KANJIDIC's radical
    bookkeeping, drop bare labels like "radical number 9", dedupe."""
    out = []
    for m in meanings:
        m = m.strip()
        if re.fullmatch(r"(radical number|variant of radical) \d+", m):
            continue
        m = re.sub(r"\s*\(no\.\s*\d+\)?$", "", m)
        m = m.removesuffix(" variant").removesuffix(" radical").strip()
        if m and m != "radical" and m not in out:
            out.append(m)
    return out


def in_scope(k):
    # Jōyō (grades 1-6 and 8) plus anything with a JLPT level.
    return k.get("grade") in {1, 2, 3, 4, 5, 6, 8} or k.get("jlpt") is not None


MAX_WORDS = 15


def part_label(p, by):
    for lit in (p["element"], p.get("original")):
        entry = by.get(lit) if lit else None
        meanings = part_meanings(entry["meanings"]) if entry else []
        name = PART_NAMES.get(lit)
        if name:
            meanings = [name] + [m for m in meanings if m != name]
        if meanings:
            return "/".join(meanings[:4])
    return "?"


def line_for(k, by):
    parts = " + ".join(f"{p['element']}={part_label(p, by)}" for p in k.get("parts", []))
    return f"{k['literal']} [{'; '.join(k['meanings'][:4])}] {parts}".rstrip()


def deck_kanji(path):
    cards = json.loads(Path(path).read_text(encoding="utf-8")).get("cards", [])
    cards = [c for c in cards if c.get("direction") == "kanji" and c.get("status") != "staging"]
    cards.sort(key=lambda c: c.get("due_ms", 0))
    return [chr(c["sequence"]) for c in cards]


def check():
    """Problems that would make a line be skipped or misread."""
    problems = []
    seen = collections.Counter()
    for n, line in enumerate(TSV.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        k, sep, phrase = line.partition("\t")
        if not sep or len(k.strip()) != 1 or not phrase.strip():
            problems.append(f"line {n}: not <kanji><TAB><phrase>: {line!r}")
            continue
        if "\t" in phrase:
            problems.append(f"line {n}: extra tab in phrase")
        if len(phrase.split()) > MAX_WORDS:
            problems.append(f"line {n}: {k} is {len(phrase.split())} words (max {MAX_WORDS})")
        seen[k.strip()] += 1
    problems += [f"{k} appears {c} times" for k, c in seen.items() if c > 1]
    return problems, sum(seen.values())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("entries", type=Path)
    ap.add_argument("--count", type=int, default=150)
    ap.add_argument("--only", help="just these kanji")
    ap.add_argument("--deck", help="app backup JSON: its kanji go first")
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    if args.check:
        problems, total = check()
        print("\n".join(problems) or f"OK: {total} phrases")
        sys.exit(1 if problems else 0)

    entries = json.loads(args.entries.read_text(encoding="utf-8"))
    by = {e["literal"]: e for e in entries}
    done = set(read_tsv(TSV))

    if args.only:
        order = list(args.only)
    else:
        freq = sorted((e for e in entries if in_scope(e)), key=lambda e: (e.get("freq") is None, e.get("freq") or 0))
        order = (deck_kanji(args.deck) if args.deck else []) + [e["literal"] for e in freq]

    shown, seen = 0, set()
    for lit in order:
        k = by.get(lit)
        if lit in seen or lit in done or not k or not k["meanings"]:
            continue
        seen.add(lit)
        print(line_for(k, by))
        shown += 1
        if shown >= args.count:
            break
    remaining = sum(1 for e in entries if in_scope(e) and e["literal"] not in done)
    print(f"# {shown} listed; {remaining} in-scope kanji still without a phrase", file=sys.stderr)


if __name__ == "__main__":
    main()
