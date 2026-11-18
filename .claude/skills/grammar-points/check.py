"""Validates data/grammar.toml, and with the bee ZIP, checks our text
doesn't repeat theirs.

  python3 -I check.py [ZIP]

Without the ZIP: fields, unique ids and patterns, one {…} mark per example,
every Tatoeba example word for word as in examples.utf (id, Japanese and
English), and related ids not written yet (a to-do list, not an error).
Length limits (meaning, explanation, notes, example length) print as
warnings: trim the entry, they don't fail the check.

With the ZIP: for each entry, any run of 8+ English words that our
meaning, explanation, notes or own translations share with bee's entries
for that pattern, and any of our own Japanese sentences (or Japanese runs
in the explanation) sharing 14+ chars, or most of the sentence, with
anything in bee's dictionary. A hit is rewritten, even when it is a
coincidence. Shorter shared runs are stock phrases and pass: the check
backs up the rule "facts in, wording out", it doesn't replace it.
"""

import importlib.util
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
REQUIRED = ["id", "seq", "pattern", "forms", "jlpt", "meaning", "formation",
            "explanation", "related", "examples"]
LEVELS = {"N5", "N4", "N3", "N2", "N1"}
# The limits SKILL.md sets: chars of meaning, words of explanation and
# notes (a Japanese run counts as one word), chars of an example's Japanese.
MAX_MEANING, MAX_WORDS, MAX_EXAMPLE = 60, 80, 26
# Overlap thresholds: English words in a row shared with the same pattern's
# entries; Japanese chars in a row shared with any entry, and the share of
# our sentence they make. Below these, shared runs are stock phrases
# ("I make it a rule to", 持ってきてよかった。) that every textbook has.
EN_RUN = 8
JA_RUN = 14
JA_SHARE = 0.6


def load_bee():
    spec = importlib.util.spec_from_file_location("bee", Path(__file__).with_name("bee.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def tatoeba():
    out = {}
    with open(ROOT / "examples.utf", encoding="utf-8") as f:
        for line in f:
            if line.startswith("A: "):
                ja, _, rest = line[3:].rstrip("\n").partition("\t")
                en, _, sid = rest.partition("#ID=")
                out[sid] = (ja, en)
    return out


def longest_shared(ours, theirs):
    """The longest run of `ours` found in `theirs`."""
    best = ""
    for i in range(len(ours)):
        # Only runs longer than the best so far are worth trying.
        lo, hi = len(best) + 1, len(ours) - i
        while lo <= hi:
            mid = (lo + hi) // 2
            if ours[i:i + mid] in theirs:
                best, lo = ours[i:i + mid], mid + 1
            else:
                hi = mid - 1
    return best


def shared_candidates(ours, theirs):
    """Which of `ours` share a JA_RUN-char run with `theirs`, and the part of
    `theirs` such a run can sit in.

    Searching all of bee's text (about 30M chars) once per probe made the
    check take minutes. A hit needs a run of at least JA_RUN chars, all of
    them chars our strings use, so only bee's runs of such chars that are
    that long can hold one: one pass over them, sliding a JA_RUN window
    against the set of our windows, finds the strings worth the exact
    search, and the exact search then runs on those runs alone.
    """
    chars = set("".join(ours))
    if not chars:
        return set(), ""
    cls = "[" + "".join(re.escape(c) for c in sorted(chars)) + "]"
    runs = re.findall(cls + "{%d,}" % JA_RUN, theirs)
    windows = {}
    for j in ours:
        for i in range(len(j) - JA_RUN + 1):
            windows.setdefault(j[i:i + JA_RUN], set()).add(j)
    found = set()
    for r in runs:
        for i in range(len(r) - JA_RUN + 1):
            hit = windows.get(r[i:i + JA_RUN])
            if hit:
                found |= hit
    return found, "\n".join(runs)


def words(t):
    return re.findall(r"[a-z']+", t.lower())


def validate(points):
    errors = []
    ids = set()
    corpus = tatoeba()
    for p in points:
        name = p.get("id", "?")
        for field in REQUIRED:
            if field not in p:
                errors.append(f"{name}: missing {field}")
        if name in ids:
            errors.append(f"{name}: duplicate id")
        ids.add(name)
        if not re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*", name):
            errors.append(f"{name}: id must be a lowercase ascii slug")
        if not isinstance(p.get("hover", True), bool):
            errors.append(f"{name}: hover must be true or false")
        if p.get("jlpt") not in LEVELS:
            errors.append(f"{name}: jlpt must be one of N5…N1")
        examples = p.get("examples", [])
        if not 2 <= len(examples) <= 5:
            errors.append(f"{name}: wants 5 examples (2–3 until extended), has {len(examples)}")
        keys = set()
        for ex in examples:
            ja = ex.get("ja", "")
            if ja.count("{") != 1 or ja.count("}") != 1:
                errors.append(f"{name}: mark the pattern once with {{…}}: {ja}")
            if ("tatoeba" in ex) == ("source" in ex):
                errors.append(f"{name}: an example has either `tatoeba` or source = \"yomeru\": {ja}")
            if "tatoeba" in ex:
                got = corpus.get(ex["tatoeba"])
                plain = ja.replace("{", "").replace("}", "")
                if got is None:
                    errors.append(f"{name}: no Tatoeba sentence {ex['tatoeba']}")
                elif got != (plain, ex.get("en")):
                    errors.append(f"{name}: {ex['tatoeba']} differs from the corpus:\n    corpus {got[0]} | {got[1]}\n    entry  {plain} | {ex.get('en')}")
            elif ex.get("source") != "yomeru":
                errors.append(f"{name}: unknown source {ex.get('source')}")
            # Reports and verdicts point at an example by this key.
            if "tatoeba" in ex:
                key = ex["tatoeba"]
                if "id" in ex:
                    errors.append(f"{name}: a Tatoeba example is keyed by its pair, no id: {ja}")
            else:
                key = ex.get("id", "")
                if not re.fullmatch(r"y[1-9][0-9]*", key):
                    errors.append(f"{name}: our own example needs id = \"y<n>\": {ja}")
            if key in keys:
                errors.append(f"{name}: two examples are keyed {key}")
            keys.add(key)
    # A spelling two entries claim is a homograph (passive and potential
    # られる): allowed only when each names the other in `homographs`, so
    # it is deliberate and the lookup shows both. Otherwise it is a mistake.
    homographs = {p.get("id"): set(p.get("homographs", [])) for p in points}
    claimed = {}
    for p in points:
        name = p.get("id")
        for f in [p.get("pattern"), *p.get("forms", [])]:
            for other in claimed.get(f, []):
                if other != name and not (other in homographs[name] and name in homographs[other]):
                    errors.append(f"{name}: form {f} is also {other}'s (homographs must name each other in homographs)")
            claimed.setdefault(f, []).append(name)
    for p in points:
        for h in p.get("homographs", []):
            if h not in ids:
                errors.append(f"{p.get('id')}: homograph {h} is not an entry")
    missing = sorted({r for p in points for r in p.get("related", [])} - ids)
    return errors, missing


def word_count(text):
    return len(re.findall(r"[A-Za-z0-9'’-]+|[぀-ヿ一-鿿]+", text))


def lengths(points):
    warnings = []
    for p in points:
        name = p.get("id", "?")
        if len(p.get("meaning", "")) > MAX_MEANING:
            warnings.append(f"{name}: meaning over {MAX_MEANING} chars")
        for field in ("explanation", "notes"):
            n = word_count(p.get(field, ""))
            if n > MAX_WORDS:
                warnings.append(f"{name}: {field} has {n} words (max {MAX_WORDS})")
        for ex in p.get("examples", []):
            plain = ex.get("ja", "").replace("{", "").replace("}", "")
            if len(plain) > MAX_EXAMPLE:
                warnings.append(f"{name}: example over {MAX_EXAMPLE} chars: {plain}")
    return warnings


def overlap(points, zip_path):
    bee = load_bee()
    want = {}
    for p in points:
        for f in [p["pattern"], *p.get("forms", [])]:
            want.setdefault(f, []).append(p["id"])
    theirs = {p["id"]: [] for p in points}
    # Our own sentences are checked against the whole dictionary: a stock
    # sentence can sit under any entry.
    all_ja = []
    for r in bee.rows(zip_path):
        t = bee.text(r[5])
        all_ja.append(re.sub(r"\s", "", t))
        hit = bee.matches(r, want)
        if hit:
            for pid in want[hit]:
                theirs[pid].append(t)
    all_ja = "\n".join(all_ja)
    ours_ja = {}
    for p in points:
        ja = [re.sub(r"[{}\s]", "", e["ja"]) for e in p["examples"] if e.get("source") == "yomeru"]
        ja += re.findall(r"[぀-ヿ一-鿿、。…〜]{10,}", p["explanation"] + p.get("notes", ""))
        ours_ja[p["id"]] = ja
    candidates, their_ja = shared_candidates(
        [j for js in ours_ja.values() for j in js], all_ja)
    hits = []
    for p in points:
        text = " ".join(theirs[p["id"]])
        their_words = words(text)
        grams = {tuple(their_words[i:i + EN_RUN]) for i in range(len(their_words) - EN_RUN + 1)}
        ours = " ".join([p["meaning"], p["explanation"], p.get("notes", "")]
                        # Tatoeba's translations are CC BY and may appear anywhere.
                        + [e.get("en", "") for e in p["examples"] if "tatoeba" not in e])
        ow = words(ours)
        for i in range(len(ow) - EN_RUN + 1):
            if tuple(ow[i:i + EN_RUN]) in grams:
                hits.append(f"{p['id']}: shares \"{' '.join(ow[i:i + EN_RUN])}\"")
        for j in ours_ja[p["id"]]:
            if j not in candidates:
                continue
            # A pattern plus a common verb (持ってきてよかった。) is shared by
            # everyone; most of a sentence isn't.
            shared = longest_shared(j, their_ja)
            if len(shared) >= max(JA_RUN, JA_SHARE * len(j)):
                hits.append(f"{p['id']}: \"{j}\" shares \"{shared}\"")
    return sorted(set(hits))


def check_seqs(points, next_seq):
    """Each entry's seq unique, from 1 to below next_seq (compile's rule)."""
    if not isinstance(next_seq, int):
        return ["next_seq missing at the top of the file"]
    errors, owner = [], {}
    for p in points:
        seq, name = p.get("seq"), p.get("id", "?")
        if not isinstance(seq, int) or not 1 <= seq < next_seq:
            errors.append(f"{name}: seq {seq} must be from 1 to {next_seq - 1}; "
                          "a new entry takes next_seq and raises it")
        elif seq in owner:
            errors.append(f"{name}: seq {seq} is already {owner[seq]}'s")
        else:
            owner[seq] = name
    return errors


def main():
    data = tomllib.loads((ROOT / "data" / "grammar.toml").read_text(encoding="utf-8"))
    points = data.get("point", [])
    errors, missing = validate(points)
    errors += check_seqs(points, data.get("next_seq"))
    for e in errors:
        print("ERROR", e)
    for w in lengths(points):
        print("LONG", w)
    by_level = {}
    for p in points:
        by_level[p.get("jlpt")] = by_level.get(p.get("jlpt"), 0) + 1
    print(f"{len(points)} points {json.dumps(by_level, ensure_ascii=False)}; {len(errors)} errors")
    if missing:
        print("related, not written yet:", " ".join(missing))
    if len(sys.argv) > 1:
        hits = overlap(points, sys.argv[1])
        for h in hits:
            print("OVERLAP", h)
        print(f"overlap check: {len(hits)} hits")
        errors += hits
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
