"""Prepares a grammar batch from a level plan, in one call.

The plan (a TOML file kept outside the repo, next to bee.zip) splits a
level's points into batches of about 20. It holds only our own decisions —
id, pattern, forms, hover, a tested Tatoeba regex, a note — and the names
of bee's rows each point covers, never bee's text.

  status   each batch: points written / planned
  show N   batch N's unwritten points: the plan's decisions, then example
           candidates from Tatoeba for each, and the `bee.py facts` calls
           to run once the entries are written
  test     every point's regex: match count, to catch a regex that
           finds nothing or everything

  python3 -I batch.py status
  python3 -I batch.py show 2 [--count 8] [--level n3]
  python3 -I batch.py test [--level n3]
"""

import argparse
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))  # -I drops the script's folder; tatoeba.py is ours
import tatoeba  # noqa: E402

GRAMMAR_TOML = HERE.parents[2] / "data" / "grammar.toml"
PLANS = Path.home() / ".cache" / "yomeru-grammar"


def written_ids():
    with open(GRAMMAR_TOML, "rb") as f:
        return {p["id"] for p in tomllib.load(f).get("point", [])}


def show(batch, done, sentences, count, level):
    print(f"# Batch {batch['n']}: {batch['title']}")
    if batch.get("note"):
        print(batch["note"].strip())
    for fold in batch.get("fold", []):
        print(f"FOLD {fold}")
    todo = [p for p in batch["point"] if p["id"] not in done]
    skipped = len(batch["point"]) - len(todo)
    if skipped:
        print(f"({skipped} already written)")
    for p in todo:
        print(f"\n## {p['id']}  {p['pattern']}  [{p.get('jlpt', level)}]")
        if p.get("forms"):
            print("forms: " + ", ".join(p["forms"]))
        if p.get("hover") is False:
            print("hover: false")
        if p.get("related"):
            print("related: " + ", ".join(p["related"]))
        if p.get("note"):
            print("note: " + p["note"].strip())
        if p.get("regex"):
            n, lines = tatoeba.pick(sentences, p["regex"], count)
            print(f"tatoeba /{p['regex']}/  {n} matches")
            for line in lines:
                print("  " + line)
        else:
            print("tatoeba: none planned, write our own")
    pats = [p.get("facts", p["pattern"]) for p in todo]
    if pats:
        print("\n# Fact sheets, after writing:")
        for i in range(0, len(pats), 5):
            print("python3 -I bee.py facts ZIP " + " ".join(pats[i:i + 5]))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["status", "show", "test"])
    ap.add_argument("n", nargs="?", type=int)
    ap.add_argument("--level", default="n4", help="picks <level>-plan.toml (default n4)")
    ap.add_argument("--plan", help="a plan file instead of --level")
    ap.add_argument("--count", type=int, default=8)
    args = ap.parse_args()

    plan_path = args.plan or PLANS / f"{args.level.lower()}-plan.toml"
    with open(plan_path, "rb") as f:
        plan = tomllib.load(f)
    done = written_ids()
    batches = plan["batch"]
    ids = [p["id"] for b in batches for p in b["point"]]
    dupes = {i for i in ids if ids.count(i) > 1}
    if dupes:
        print("duplicate ids in plan: " + ", ".join(sorted(dupes)))

    if args.cmd == "status":
        for b in batches:
            pts = b["point"]
            n = sum(p["id"] in done for p in pts)
            print(f"batch {b['n']}: {n}/{len(pts)}  {b['title']}")
        return

    sentences = tatoeba.load()
    if args.cmd == "test":
        for b in batches:
            for p in b["point"]:
                if not p.get("regex"):
                    continue
                n, _ = tatoeba.pick(sentences, p["regex"], 0)
                flag = "  <-- none" if n == 0 else "  <-- broad" if n > 3000 else ""
                print(f"{b['n']} {p['id']:24} {n:6}{flag}")
        return

    batch = next((b for b in batches if b["n"] == args.n), None)
    if batch is None:
        sys.exit(f"no batch {args.n}; batches: {[b['n'] for b in batches]}")
    show(batch, done, sentences, args.count, plan.get("level", args.level.upper()))


if __name__ == "__main__":
    main()
