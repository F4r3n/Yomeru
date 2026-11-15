"""Renders data/grammar.toml as one HTML page, to review entries as a
learner would see them.

  python3 -I preview.py OUT.html [--bee ZIP]

Our text and Tatoeba's only (the TOML holds nothing else). With --bee,
each entry also links to bee-san's page for the same grammar point, to
compare while reviewing: a link to their site, none of their content.
"""

import argparse
import html
import importlib.util
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
LEVELS = ["N5", "N4", "N3", "N2", "N1"]
BEE_PAGE = "https://bee-san.github.io/bees-ultimate-grammar-dictionary/grammar/{}/"


def bee_links(points, zip_path):
    """id → [(bee headword, page url)] for the grammar points bee has
    under our pattern or forms."""
    spec = importlib.util.spec_from_file_location("bee", Path(__file__).with_name("bee.py"))
    bee = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bee)
    owner = {}
    for p in points:
        for f in [p["pattern"], *p.get("forms", [])]:
            owner.setdefault(f, p["id"])
    links = {p["id"]: [] for p in points}
    for r in bee.rows(zip_path):
        hit = bee.matches(r, owner)
        pid = bee.point_id(r)
        if hit and re.fullmatch(r"[0-9a-f]{20}", pid):
            entry = (r[0], BEE_PAGE.format(pid))
            if entry not in links[owner[hit]]:
                links[owner[hit]].append(entry)
    return links

CSS = """
/* Layout: an index rail beside one column of entries; the rail folds into
   a strip of chips on phones. Colours are Yomeru's own (Catppuccin
   Latte / Mocha), so the page reads like the app. */
:root {
  --bg: #eff1f5; --surface: #e6e9ef; --card: #ffffff; --border: #bcc0cc;
  --text: #4c4f69; --subtext: #6c6f85; --accent: #8839ef; --on-accent: #ffffff;
  --mark: #df8e1d33; --mark-text: #4c4f69; --green: #40a02b; --blue: #04a5e5;
  --ja: "Noto Sans JP", "Hiragino Sans", "Yu Gothic", sans-serif;
  --en: "Inter", "Segoe UI", system-ui, sans-serif;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #181825; --surface: #1e1e2e; --card: #24243a; --border: #45475a;
  --text: #cdd6f4; --subtext: #a6adc8; --accent: #cba6f7; --on-accent: #1e1e2e;
  --mark: #f9e2af2e; --mark-text: #f9e2af; --green: #a6e3a1; --blue: #89dceb;
  color-scheme: dark; } }
:root[data-theme="dark"] {
  --bg: #181825; --surface: #1e1e2e; --card: #24243a; --border: #45475a;
  --text: #cdd6f4; --subtext: #a6adc8; --accent: #cba6f7; --on-accent: #1e1e2e;
  --mark: #f9e2af2e; --mark-text: #f9e2af; --green: #a6e3a1; --blue: #89dceb;
  color-scheme: dark; }

* { box-sizing: border-box; }
body { background: var(--bg); color: var(--text); font: 15px/1.6 var(--en); margin: 0; }
.wrap { max-width: 1120px; margin: 0 auto;
  padding-left: max(16px, env(safe-area-inset-left, 0px));
  padding-right: max(16px, env(safe-area-inset-right, 0px));
  padding-block: 28px 64px; }
[lang="ja"] { font-family: var(--ja); }
a { color: var(--accent); }
a:focus-visible, button:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

header.top { display: flex; flex-wrap: wrap; align-items: end; justify-content: space-between; gap: 16px; margin-bottom: 24px; }
header.top h1 { margin: 0; font-size: 26px; letter-spacing: -0.01em; text-wrap: balance; }
header.top h1 span { font-family: var(--ja); color: var(--accent); margin-right: 8px; }
.counts { display: flex; gap: 6px; flex-wrap: wrap; font-size: 12px; color: var(--subtext); }
.counts b { color: var(--text); font-variant-numeric: tabular-nums; }

.tools { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-bottom: 20px; }
.tools input { flex: 1 1 220px; min-width: 0; padding: 9px 12px; border-radius: 8px; border: 1px solid var(--border);
  background: var(--card); color: var(--text); font: inherit; }
.lv { border: 1px solid var(--border); background: transparent; color: var(--subtext); border-radius: 999px;
  padding: 5px 12px; font: 600 12px var(--en); letter-spacing: 0.04em; cursor: pointer; }
.lv[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }

.layout { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 32px; align-items: start; }
nav.index { position: sticky; top: calc(env(safe-area-inset-top, 0px) + 16px); max-height: calc(100vh - 32px); overflow-y: auto;
  font-size: 14px; }
nav.index h2 { font: 600 11px var(--en); letter-spacing: 0.08em; text-transform: uppercase; color: var(--subtext); margin: 16px 0 6px; }
nav.index h2:first-child { margin-top: 0; }
nav.index a { display: flex; justify-content: space-between; gap: 8px; padding: 3px 8px; border-radius: 6px;
  color: var(--text); text-decoration: none; }
nav.index a:hover { background: var(--surface); }
nav.index a small { color: var(--subtext); font-family: var(--en); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

main { display: flex; flex-direction: column; gap: 20px; min-width: 0; }
article { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 22px 24px; scroll-margin-top: 16px; }
article:target { border-color: var(--accent); }
.head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px; }
.pattern { font: 700 30px/1.2 var(--ja); color: var(--text); }
.badge { font: 600 11px var(--en); letter-spacing: 0.06em; border: 1px solid var(--border); border-radius: 999px;
  padding: 2px 9px; color: var(--subtext); align-self: center; }
.forms { font: 13px var(--ja); color: var(--subtext); }
.meaning { font-size: 17px; margin: 6px 0 16px; }

.section { margin-top: 18px; }
.label { font: 600 11px var(--en); letter-spacing: 0.08em; text-transform: uppercase; color: var(--subtext); margin-bottom: 6px; }
.formation { display: flex; flex-direction: column; gap: 6px; }
.rule { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
.piece { font: 14px var(--ja); background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 1px 8px; }
.piece.grammar { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); }
.plus { color: var(--subtext); font-size: 13px; }
.piece .tag { color: var(--blue); }
.rule .eg { font: 13px var(--ja); color: var(--subtext); }

p.text { margin: 0; max-width: 68ch; }
.notes { background: var(--surface); border-radius: 8px; padding: 12px 14px; max-width: 72ch; }
.notes p { margin: 0; }

.examples { display: flex; flex-direction: column; gap: 12px; }
.ex { border-left: 2px solid var(--border); padding-left: 12px; }
.ex .ja { font: 17px/1.7 var(--ja); }
.ex mark { background: var(--mark); color: var(--mark-text); border-radius: 3px; padding: 0 2px; font-weight: 600; }
.ex .en { color: var(--subtext); font-size: 14px; }
.ex .src { font-size: 11px; color: var(--subtext); letter-spacing: 0.02em; }
.ex .src a { color: var(--subtext); }

.related { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { font: 13px var(--ja); border: 1px solid var(--border); border-radius: 999px; padding: 2px 10px;
  color: var(--text); text-decoration: none; background: var(--card); }
a.chip:hover { border-color: var(--accent); }
.chip.bee { color: var(--subtext); font-size: 12px; }
.chip.todo { color: var(--subtext); border-style: dashed; font-family: var(--en); font-size: 12px; }

.empty { color: var(--subtext); padding: 24px 0; }
footer { margin-top: 40px; font-size: 12px; color: var(--subtext); max-width: 72ch; }

@media (max-width: 760px) {
  .layout { grid-template-columns: minmax(0, 1fr); gap: 16px; }
  nav.index { position: static; max-height: none; display: flex; flex-wrap: wrap; gap: 6px; }
  nav.index h2 { display: none; }
  nav.index a { border: 1px solid var(--border); border-radius: 999px; padding: 2px 10px; font-family: var(--ja); }
  nav.index a small { display: none; }
  article { padding: 18px 16px; }
  .pattern { font-size: 26px; }
}
@media (prefers-reduced-motion: no-preference) { html { scroll-behavior: smooth; } }
"""

SCRIPT = """
(function () {
  const q = document.getElementById('q');
  const buttons = [...document.querySelectorAll('.lv')];
  const arts = [...document.querySelectorAll('article')];
  const links = [...document.querySelectorAll('nav.index a')];
  const empty = document.getElementById('empty');
  let level = 'all';
  function apply() {
    const term = q.value.trim().toLowerCase();
    let shown = 0;
    arts.forEach(a => {
      const ok = (level === 'all' || a.dataset.level === level) && (!term || a.dataset.search.includes(term));
      a.hidden = !ok; if (ok) shown++;
    });
    links.forEach(l => { const a = document.getElementById(l.dataset.id); l.hidden = a.hidden; });
    empty.hidden = shown > 0;
  }
  buttons.forEach(b => b.addEventListener('click', () => {
    level = b.dataset.level;
    buttons.forEach(x => x.setAttribute('aria-pressed', String(x === b)));
    apply();
  }));
  q.addEventListener('input', apply);
})();
"""

GRAMMAR_WORD = re.compile(r"[぀-ヿ一-鿿]")


def esc(s):
    return html.escape(s, quote=True)


def mark(ja):
    """`{…}` → <mark>."""
    return re.sub(r"\{(.*?)\}", lambda m: f"<mark>{m.group(1)}</mark>", esc(ja))


def rule(line):
    """`V-て + もいい   (example)` → pieces joined by +."""
    body, _, eg = line.partition("(")
    pieces = [p.strip() for p in re.split(r"\s\+\s|\s→\s|,\s", body) if p.strip()]
    seps = re.findall(r"\s(\+|→)\s|(,)\s", body)
    out = []
    for i, p in enumerate(pieces):
        # The part learners add (the grammar) is in kana with no "-adj"/"V-" tag.
        grammar = GRAMMAR_WORD.search(p) and not re.search(r"[A-Za-z]", p)
        # Bunpro-style form tags (Verb［て］, ［い］Adjective) stand out.
        body_html = re.sub(r"［(.*?)］", r'<span class="tag">［\1］</span>', esc(p))
        out.append(f'<span class="piece{" grammar" if grammar else ""}" lang="ja">{body_html}</span>')
        if i < len(seps):
            sep = seps[i][0] or seps[i][1]
            out.append(f'<span class="plus">{esc(sep)}</span>')
    if eg:
        out.append(f'<span class="eg">({esc(eg.rstrip(") "))})</span>')
    return f'<div class="rule">{"".join(out)}</div>'


def example(ex):
    if "tatoeba" in ex:
        ja_id = ex["tatoeba"].split("_")[0]
        src = f'Tatoeba <a href="https://tatoeba.org/en/sentences/show/{esc(ja_id)}" target="_blank" rel="noopener">#{esc(ja_id)}</a>'
    else:
        src = "Written for Yomeru"
    return (f'<div class="ex"><div class="ja" lang="ja">{mark(ex["ja"])}</div>'
            f'<div class="en">{esc(ex["en"])}</div><div class="src">{src}</div></div>')


def article(p, by_id, bee):
    forms = "・".join(p.get("forms", []))
    related = []
    for rid in p.get("related", []):
        if rid in by_id:
            related.append(f'<a class="chip" href="#{esc(rid)}" lang="ja">{esc(by_id[rid]["pattern"])}</a>')
        else:
            related.append(f'<span class="chip todo" title="Not written yet">{esc(rid)}</span>')
    search = " ".join([p["pattern"], *p.get("forms", []), p["meaning"], p["id"]]).lower()
    parts = [
        f'<article id="{esc(p["id"])}" data-level="{esc(p["jlpt"])}" data-search="{esc(search)}">',
        '<div class="head">',
        f'<span class="pattern" lang="ja">{esc(p["pattern"])}</span>',
        f'<span class="badge">{esc(p["jlpt"])}</span>',
        f'<span class="forms" lang="ja">{esc(forms)}</span>' if forms else "",
        "</div>",
        f'<div class="meaning">{esc(p["meaning"])}</div>',
        '<div class="section"><div class="label">Formation</div><div class="formation">',
        *[rule(f) for f in p["formation"]],
        "</div></div>",
        f'<div class="section"><div class="label">How it works</div><p class="text">{esc(p["explanation"].strip())}</p></div>',
    ]
    if p.get("notes"):
        parts.append(f'<div class="section"><div class="label">Watch out</div><div class="notes"><p>{esc(p["notes"].strip())}</p></div></div>')
    parts.append('<div class="section"><div class="label">Examples</div><div class="examples">')
    parts += [example(e) for e in p["examples"]]
    parts.append("</div></div>")
    if related:
        parts.append(f'<div class="section"><div class="label">Compare with</div><div class="related">{"".join(related)}</div></div>')
    if bee.get(p["id"]):
        anchors = "".join(
            f'<a class="chip bee" href="{esc(url)}" target="_blank" rel="noopener" lang="ja">{esc(name)} ↗</a>'
            for name, url in bee[p["id"]])
        parts.append(f'<div class="section"><div class="label">Compare on bee-san</div><div class="related">{anchors}</div></div>')
    parts.append("</article>")
    return "\n".join(x for x in parts if x)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--bee", help="bee-san ZIP, to link each entry to their page")
    args = ap.parse_args()
    out = Path(args.out)
    data = tomllib.loads((ROOT / "data" / "grammar.toml").read_text(encoding="utf-8"))
    points = sorted(data.get("point", []), key=lambda p: LEVELS.index(p["jlpt"]))
    by_id = {p["id"]: p for p in points}
    bee = bee_links(points, args.bee) if args.bee else {}
    counts = {lv: sum(p["jlpt"] == lv for p in points) for lv in LEVELS}
    levels = [lv for lv in LEVELS if counts[lv]]

    index = []
    for lv in levels:
        index.append(f"<h2>{lv}</h2>")
        for p in points:
            if p["jlpt"] == lv:
                short = p["meaning"].split(",")[0]
                index.append(f'<a href="#{esc(p["id"])}" data-id="{esc(p["id"])}"><span lang="ja">{esc(p["pattern"])}</span><small>{esc(short)}</small></a>')

    page = f"""<title>Yomeru Grammar Points</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700&family=Noto+Sans+JP:wght@400;600;700&display=swap">
<style>{CSS}</style>
<div class="wrap">
<header class="top">
  <h1><span lang="ja">文法</span>Yomeru grammar points</h1>
  <div class="counts">{" · ".join(f"<span>{lv} <b>{counts[lv]}</b></span>" for lv in levels)} · <span>total <b>{len(points)}</b></span></div>
</header>
<div class="tools">
  <input id="q" type="search" placeholder="Search a pattern or a meaning…" aria-label="Search">
  <button class="lv" data-level="all" aria-pressed="true">All</button>
  {"".join(f'<button class="lv" data-level="{lv}" aria-pressed="false">{lv}</button>' for lv in levels)}
</div>
<div class="layout">
<nav class="index" aria-label="Grammar points">{"".join(index)}</nav>
<main>
{"".join(article(p, by_id, bee) for p in points)}
<div id="empty" class="empty" hidden>No grammar point matches.</div>
</main>
</div>
<footer>Explanations and the sentences marked “Written for Yomeru” are Yomeru's own.
Sentences marked Tatoeba come from the <a href="https://tatoeba.org" target="_blank" rel="noopener">Tatoeba</a> corpus,
licensed CC BY 2.0 FR.{" The “Compare on bee-san” links open Bee's Ultimate Grammar Dictionary, for review only." if bee else ""} JLPT levels are the usual textbook placement; the official lists stopped in 2010.</footer>
</div>
<script>{SCRIPT}</script>
"""
    out.write_text(page, encoding="utf-8")
    print(f"wrote {out} ({len(points)} points)")


if __name__ == "__main__":
    main()
