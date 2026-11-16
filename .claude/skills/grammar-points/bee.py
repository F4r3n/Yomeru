"""Reads bee-san's grammar dictionary ZIP for the grammar-points skill.

The ZIP is a checklist and a fact-checker, never a source of text: its
content belongs to the 14 sources it collects. Keep it outside the repo,
and never paste what this prints into data/grammar.toml.

  list   grammar points of the given JLPT levels that data/grammar.toml
         doesn't cover yet, with bee's level and structure line
  facts  for each pattern, a short English fact sheet: the explanations and
         caveats of the sources, without their examples and dialogues,
         capped so a review stays cheap

  python3 -I bee.py list  ZIP --levels N5 --count 20 [--reviewed]
  python3 -I bee.py facts ZIP てもいい ようにする [--cap 6000]
"""

import argparse
import json
import re
import sys
import tomllib
import zipfile
from pathlib import Path

GRAMMAR_TOML = Path(__file__).resolve().parents[3] / "data" / "grammar.toml"
# Bee rows already looked at that need no entry of their own, kept outside
# the repo like the plans: `"<bee headword>" = "<our id>"` when one of ours
# covers it under another spelling, or `= "out: <reason>"` when it's left
# out on purpose. `list` hides them; `list --reviewed` shows only them.
REVIEWED_TOML = Path.home() / ".cache" / "yomeru-grammar" / "reviewed.toml"

# Sources whose explanations are short and fact-dense come first; IMABI's
# lessons are long essays that wander into side topics, so it comes last
# and gets a small share.
SOURCE_ORDER = [
    "どんなときどう使う 日本語表現文型辞典",
    "日本語文型辞典 英語版",
    "DoJG 日本語文法辞典(全集)",
    "Bunpro Grammar Reference",
    "AIUEO JLPT Grammar",
    "くらべてわかる日本語表現文型辞典",
    "IMABI",
]
SOURCE_SHARE = {"IMABI": 900}
# Roles holding an explanation's text.
PROSE = {"prose", "proseTranslation", "proseHeading", "senseLabel"}
# Roles never read: examples, dialogues, pictures.
SKIP = {"examples", "example", "inlineExample", "exampleDerivation",
        "exampleAnnotation", "illustration", "patterns"}


def rows(zip_path):
    with zipfile.ZipFile(zip_path) as z:
        for name in sorted(z.namelist()):
            if re.fullmatch(r"term_bank_\d+\.json", name):
                yield from json.loads(z.read(name))


def roles(node):
    """The node's `data` roles; a node may carry several."""
    data = node.get("data") if isinstance(node, dict) else None
    return set(data) if isinstance(data, dict) else set()


def text(node):
    if isinstance(node, str):
        return node
    if isinstance(node, list):
        return "".join(text(n) for n in node)
    if isinstance(node, dict):
        if node.get("tag") == "rt":
            return ""
        return text(node.get("content", ""))
    return ""


def find(node, want):
    if isinstance(node, dict):
        if want in roles(node):
            return node
        return find(node.get("content"), want)
    if isinstance(node, list):
        for n in node:
            hit = find(n, want)
            if hit is not None:
                return hit
    return None


def point_id(row):
    m = re.findall(r"grammar/([0-9a-f]{20})", json.dumps(row[5]))
    return m[-1] if m else row[0]


def level(row):
    j = find(row[5], "jlpt")
    m = re.search(r"N[1-5]", text(j)) if j is not None else None
    return m.group(0) if m else ""


def bunpro_level(row):
    """Bunpro's level for the row's point, the level our entries take."""
    m = re.search(r"Bunpro Grammar ReferenceJLPT (N[1-5])", text(row[5]))
    return m.group(1) if m else ""


def covered():
    """Patterns and forms already in data/grammar.toml."""
    if not GRAMMAR_TOML.exists():
        return set()
    d = tomllib.loads(GRAMMAR_TOML.read_text(encoding="utf-8"))
    names = {f for p in d.get("point", []) for f in [p["pattern"], *p.get("forms", [])]}
    # Bee's names lose their 〜 in core_names(): ours must match the same way.
    return names | {n for f in names for n in core_names(f)}


def core_names(name):
    """The pattern(s) a bee headword names. Some are descriptive
    ("Verb + てもいい", "AながらB", "The Particle ながら I: …") or list
    variants ("そうに・そうな"): Latin words, placeholders and signs are
    dropped and each variant is returned. Bracketed text stays part of the
    name, since it often names another point: ながら（も） and ながら（逆接）
    are not ながら. Never a substring match either."""
    stripped = re.sub(r"[A-Za-z0-9\s+＋:：［］\[\]〜～…]+", "", name or "")
    return [part for part in stripped.split("・") if part]


def is_covered(name, have):
    """Every pattern `name` names is one of ours (a pattern or a form)."""
    parts = core_names(name)
    return bool(parts) and all(part in have for part in parts)


def matches(row, patterns):
    """The pattern among `patterns` this row is an entry for, if any."""
    for name in (row[0], row[1]):
        if name in patterns:
            return name
        for part in core_names(name):
            if part in patterns:
                return part
    return None


def reviewed():
    """Bee headword -> our id or "out: reason". Warns on unknown ids."""
    if not REVIEWED_TOML.exists():
        return {}
    rev = tomllib.loads(REVIEWED_TOML.read_text(encoding="utf-8"))
    ids = set()
    if GRAMMAR_TOML.exists():
        d = tomllib.loads(GRAMMAR_TOML.read_text(encoding="utf-8"))
        ids = {p["id"] for p in d.get("point", [])}
    for name, v in rev.items():
        if not v.startswith("out:") and v not in ids:
            print(f"reviewed.toml: {name!r} points at unknown id {v!r}", file=sys.stderr)
    return rev


def cmd_list(args):
    have = covered()
    rev = reviewed()
    seen, out = set(), []
    for r in rows(args.zip):
        pid = point_id(r)
        if pid in seen:
            continue
        seen.add(pid)
        lv = level(r)
        if lv not in args.levels or is_covered(r[0], have) or is_covered(r[1], have):
            continue
        note = rev.get(r[0])
        if (note is not None) != args.reviewed:
            continue
        s = find(r[5], "structure")
        bp = bunpro_level(r)
        out.append((lv, r[0], r[1], text(s).strip() if s is not None else "",
                    f"  [Bunpro {bp}]" if bp else "") + ((f"  -> {note}",) if note else ("",)))
    order = {lv: i for i, lv in enumerate(["N5", "N4", "N3", "N2", "N1"])}
    out.sort(key=lambda x: order[x[0]])
    what = "reviewed" if args.reviewed else "not covered yet"
    print(f"{len(out)} {what} in {','.join(args.levels)}", file=sys.stderr)
    for lv, term, reading, structure, bp, note in out[: args.count]:
        extra = f" ({reading})" if reading and reading != term else ""
        print(f"{lv}  {term}{extra}  {structure}{bp}{note}")


def blocks(node, source, out):
    """Collects (source, paragraph) pairs, skipping examples."""
    if isinstance(node, list):
        for n in node:
            blocks(n, source, out)
        return
    if not isinstance(node, dict):
        return
    r = roles(node)
    if r & SKIP:
        return
    if "sourceBlock" in r:
        name = find(node, "sourceName")
        source = text(name).strip() if name is not None else "?"
    if r & PROSE:
        t = clean(text(node))
        if t:
            out.append((source, t))
        return
    blocks(node.get("content"), source, out)


# A Japanese sentence (an example or a line of dialogue) inside a
# paragraph, and dialogue speaker marks.
JA_SENTENCE = re.compile(r"[^\x00-\x7F]*[\u3040-\u30ff\u4e00-\u9fff][^\x00-\x7F]*[。？！]")
SPEAKER = re.compile(r"(?<![A-Za-z])[ABＡＢ][:：]\s*")


def clean(t):
    t = SPEAKER.sub("", JA_SENTENCE.sub(" ", t))
    return re.sub(r"\s+", " ", t).strip()


def english(t):
    letters = sum(c.isascii() and c.isalpha() for c in t)
    return letters >= max(12, len(t) // 3)


def cmd_facts(args):
    want = set(args.patterns)
    found = {p: [] for p in args.patterns}
    for r in rows(args.zip):
        hit = matches(r, want)
        if hit:
            found[hit].append(r)
    for pattern, rs in found.items():
        print(f"######## {pattern}")
        if not rs:
            print("(not in the dictionary)")
            continue
        for r in rs:
            s = find(r[5], "structure")
            print(f"== {r[0]} {r[1]}  level {level(r) or '-'}  structure {text(s).strip() if s is not None else '-'}")
        paras = []
        for r in rs:
            blocks(r[5], "?", paras)
        # Each paragraph once, English only, grouped by source.
        by_source, seen = {}, set()
        for source, t in paras:
            if t in seen or not english(t):
                continue
            seen.add(t)
            by_source.setdefault(source, []).append(t)
        names = sorted(by_source, key=lambda n: SOURCE_ORDER.index(n) if n in SOURCE_ORDER else 99)
        left = args.cap
        for name in names:
            share = min(left, SOURCE_SHARE.get(name, left))
            if share < 80:
                break
            body = " / ".join(by_source[name])
            if len(body) > share:
                body = body[:share].rsplit(" ", 1)[0] + " …"
            print(f"-- {name}: {body}")
            left -= len(body)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("list")
    a.add_argument("zip")
    a.add_argument("--levels", nargs="+", default=["N5"])
    a.add_argument("--count", type=int, default=20)
    a.add_argument("--reviewed", action="store_true",
                   help="show only the rows reviewed.toml hides, with their note")
    a.set_defaults(fn=cmd_list)
    f = sub.add_parser("facts")
    f.add_argument("zip")
    f.add_argument("patterns", nargs="+")
    f.add_argument("--cap", type=int, default=4500, help="chars of prose per pattern")
    f.set_defaults(fn=cmd_facts)
    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
