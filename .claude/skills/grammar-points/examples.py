"""Prepares an examples batch: entries of a level still short of 5 examples.

Production cards show a different sentence each time, so every entry grows
to 5 examples. This splits a level's short entries into batches of about
20, in file order, and for each prints what it has, the key the next
sentence of ours takes, and Tatoeba candidates found by the entry's pattern
and forms (sentences already in the entry left out).

  status   each level: entries at 5 / all
  show N   batch N of the level's short entries

  python3 -I examples.py status
  python3 -I examples.py show 1 --level n5 [--count 8] [--size 20]
"""

import argparse
import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))  # -I drops the script's folder; tatoeba.py is ours
import tatoeba  # noqa: E402

GRAMMAR_TOML = HERE.parents[2] / "data" / "grammar.toml"
WANT = 5


def points():
    with open(GRAMMAR_TOML, "rb") as f:
        return tomllib.load(f).get("point", [])


def next_key(p):
    """`y<n>` above every number the entry uses."""
    used = [int(e["id"][1:]) for e in p["examples"] if "id" in e]
    return f"y{max(used, default=0) + 1}"


def status(ps):
    for level in ["N5", "N4", "N3", "N2", "N1"]:
        at = [p for p in ps if p["jlpt"] == level]
        done = sum(len(p["examples"]) >= WANT for p in at)
        print(f"{level}: {done}/{len(at)} at {WANT}")


def show(ps, n, level, size, count):
    short = [p for p in ps if p["jlpt"] == level.upper() and len(p["examples"]) < WANT]
    batches = [short[i:i + size] for i in range(0, len(short), size)]
    if not 1 <= n <= len(batches):
        sys.exit(f"{level}: {len(batches)} batches of short entries")
    sentences = tatoeba.load()
    print(f"# {level.upper()} examples, batch {n}/{len(batches)}")
    for p in batches[n - 1]:
        print(f"\n## {p['id']}  {p['pattern']}  — {p['meaning']}")
        print(f"   needs {WANT - len(p['examples'])}, ours take {next_key(p)}…")
        for e in p["examples"]:
            print(f"   {e.get('tatoeba') or e['id']}  {e['ja']} | {e['en']}")
        used = {e["tatoeba"] for e in p["examples"] if "tatoeba" in e}
        spellings = sorted({p["pattern"], *p.get("forms", [])}, key=len, reverse=True)
        regex = "|".join(re.escape(s) for s in spellings)
        found, lines = tatoeba.pick(sentences, regex, count + len(used))
        lines = [l for l in lines if l.split(" ", 1)[0] not in used][:count]
        print(f"   tatoeba: {found} matches on the pattern and forms")
        for line in lines:
            print(f"     {line}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["status", "show"])
    ap.add_argument("n", nargs="?", type=int, default=1)
    ap.add_argument("--level", default="n5")
    ap.add_argument("--size", type=int, default=20)
    ap.add_argument("--count", type=int, default=8)
    args = ap.parse_args()
    ps = points()
    if args.cmd == "status":
        status(ps)
    else:
        show(ps, args.n, args.level, args.size, args.count)


if __name__ == "__main__":
    main()
