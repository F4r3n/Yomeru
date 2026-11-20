---
name: sentence-split
description: Improve the sentence breakdown (the Sentence tab's word split and grammar points) from a Japanese text the user pastes. Run the text through the segmenter and grammar analysis, read every sentence for wrong splits, wrong entries and wrong or missing grammar points, then fix them one category at a time, each change measured against the Tanaka corpus. Trigger when the user pastes Japanese text to check the sentence split / breakdown / segmentation, or asks to verify or improve how sentences are split or analyzed.
---

# Sentence split improvement

The Sentence tab shows a sentence's words (`crates/jmdict-core/src/segment.rs`,
a cost lattice) and its grammar points (`crates/sentence-core/src/lib.rs`, with
`data/grammar.toml`). A pasted text is a test set: find where the breakdown is
wrong, then fix the code without making the corpus worse.

## Tools (this folder)

Everything uses the full dictionary at `~/.cache/yomeru-sentence/jmdict-full.bin`
(a `jmdict-types/full` build: the reports need it). Missing? `./setup.sh` builds it
(and fetches `JMdict_e` / `examples.utf` if absent). Reports go in the same folder.

- `split.sh FILE`: the breakdown of every sentence of FILE: words (with the
  deinflection: `なった(なる Past)`) and grammar points (`Text`, `Conjugation`,
  `Slot`, `Read` + the point's id), and `phrase:` lines: an expression JMdict
  lists whole (と言った, 雨が降る) shown split, with its split.
- `score.sh TAG [COSTS]`: corpus scores; writes `seg-TAG.tsv`, `gram-TAG.tsv`.
  - segmentation: word / boundary F1, right entry on matched words, sentences
    split exactly as the corpus; then the same **conventions aside** (では vs
    で|は, に|なる, 成功|する, 行き|たい, the gold's leading digits): the line
    to watch, since most raw disagreements are the corpus' conventions.
    `seg-TAG.tsv` lists only the rest;
  - grammar: examples of `grammar.toml` found exactly / overlap / missed, and
    other matches per sentence (noise: lower is better); then the same with
    **conventions** aside (our span holds the mark or sits in it, and is the
    entry's pattern as written, conjugated, or its slots: の間に for a mark
    間に). `gram-TAG.tsv` lists only the rest.
  `COSTS` (`word=900,particle_head=1200`) tries other `segment::Costs` values
  without editing code: use it to sweep a new cost before fixing its value.
- `compare.sh OLD NEW [N]`: what moved between two runs: corpus words newly
  wrong (`>`) and newly right (`<`), counted, then the grammar examples that
  changed. The scores say *whether*, this says *what*.
- `probe.sh words=… at=… sentences=… costs=…`:
  - `words=とき,時`: entries with that spelling, frequency rank (`pri`, 0 is
    the commonest) and classes;
  - `at=文:3`: every candidate word starting at char 3 of 文, with its cost
    (the node cost only; connections come on top);
  - `sentences=文`: the lattice's split with each word's class.

The scripts run `cargo test --release`: about 1–2 min for `score.sh`. Run
long ones with a generous timeout.

## Workflow

1. **Save the text** to the scratchpad (`text.txt`) and run `split.sh` on it.
2. **Read every sentence as a native reader would.** List what's wrong:
   - **A, segmentation**: a word split wrong (実行され as one word, な|どの
     for など|の) or the right split with the wrong entry (担う for に|なった,
     とき as 解く instead of 時, 通う for 通る);
   - **B, grammar points misplaced**: a point found where it doesn't apply
     (から "from" inside だから, にしろ "whether" on にする + imperative);
   - **C, grammar points missing**: a point that's there but not found, or not
     written yet (then it's a grammar-points skill entry).
   Show the user the list grouped by category, each with the sentence, what
   it shows and what it should, and a guess at the cause. **Ask which to fix**
   before editing (the user wants to discuss first), and commit between
   categories when they say so (commit-with-jj skill).
3. **Baseline**: `score.sh base` before any change.
4. **Per issue**: diagnose → change → measure → keep or revert.
   - Diagnose with `probe.sh`: compare the costs of the wrong path and the
     right one (`at=` at each word start). Each word's node cost plus the
     connection costs (`Costs::connection`) decides; ties go either way.
   - Change the smallest thing that explains the issue (see **Where fixes
     go**).
   - Measure: `score.sh <tag>`, then `compare.sh base <tag>`, and re-run
     `split.sh` (or a `BREAKDOWN` of the one sentence) to see it fixed.
   - **Keep** a change only when the sentence is fixed and the corpus isn't
     worse: F1, exact split and grammar exact within 0.1 of the baseline,
     and the newly wrong rows in `compare.sh` aren't real regressions.
     **Revert** it otherwise and try a narrower condition; after two or three
     tries, report it as a known limit.
   - The corpus has its own conventions (とした is one word, とする; のうち
     can be either): when a fix is right for the text but the corpus reads
     the other way, the lattice can't tell them apart without context; don't
     force it, list it.
   - Add a unit test for each kept fix where one fits: `deinflect` rules
     (`crates/deinflect/src/lib.rs` tests), grammar flags and filters
     (`crates/sentence-core/src/tests.rs`, with its small fixture dictionary:
     add the words a test needs).
5. **Finish**: `cargo xtask test`, `cargo clippy -q -p <touched crates>
   --all-targets` (the wasm-only app crates have their own clippy errors on
   the host; ignore those), restart the API server if it's running (TaskStop,
   then the run-web-dev command) so the UI shows the change.
6. **Report**: a table of each issue (sentence, before, now), the scores
   before/after, the attempts reverted and why, what's still open. Don't
   commit until the user asks.

## Where fixes go

Segmentation (`segment.rs`):
- **No candidate** for the right word (書か before ざる, 通ってる): candidate
  generation: `extra_forms` (stems the deinflector doesn't give), or a
  rule in `crates/deinflect/src/rules.rs`. A deinflect rule applies
  everywhere (the popup, the grammar lookup): check it doesn't swallow
  something a grammar point needs (a ざる → ず rule made 辞めざる one word
  and broke ざるを得ない's matches).
- **Wrong path cheaper**: a cost. `Costs` holds them (with a `with()` key
  each, so `score.sh TAG key=N` sweeps it); `connection(prev, node)` adds
  costs by the previous word's class; node costs come from `push_nodes`
  (frequency, kana spelling of a kanji word, extra form).
  Things learned the hard way:
  - a broad condition breaks fixed expressions: exempt
    `Class::Expression` (では|ない, に|なった);
  - frequency tags can sit on the kanji spelling only (時 tagged, とき not);
  - nouns, verbs and adjectives also take IPADIC's word cost (`word_cost`,
    `ipadic_base` / `ipadic_scale`), when lower than their tags give; not
    adverbs, conjunctions or expressions, which made cheap swallow a noun
    and its particle (時に, なので, 電|気がつく). `ipadic_scale=0` is the
    tags alone, to see what IPADIC changes;
  - the corpus keeps some kanji + particle expressions whole (人気がある,
    に対して) and splits others (と言った, 雨が降る): no cost can follow it
    (`phrase` past ~800 is a cliff). The breakdown splits them for display
    instead (`segment::split_phrase`, `Breakdown::phrases`), after the
    grammar is read on the whole expression; the corpus scores the lattice
    and doesn't see it;
  - the class a word leaves in the lattice decides the next connection
    (a する noun read with its する must leave `Verb`, or とき after
    アクセスできない pays the noun-after-noun cost);
  - a node flag on a whole class can hit an innocent word of it (とき is
    kana for 時, so a "kana for kanji" cost hit it too).
Grammar (`sentence-core`, `data/grammar.toml`):
- Prefer per-point data over code: `after` (what it follows), `final`,
  `continues`, `no_suru`, `homographs`. Edit `grammar.toml` the way the
  grammar-points skill says (python `str.replace` with count asserts, never
  Read the whole file); `check.py` in that skill validates it.
- A general rule in `lib.rs` (a filter for every point) is a last resort: it
  failed before (に before する dropped for all points broke 抜きにしましょう).

## Last scores

2026-11-20, with IPADIC's word costs and `phrase` 700, after the
てくれた fix: segmentation F1 89.3, boundaries 95.2, right entry 97.7%,
exact 58.9%; conventions aside F1 96.3, boundaries 98.3, exact 85.1%;
grammar exact 89.0%, conventions aside 97.2% (overlap 0.1, missed 2.7),
other matches 2.76 per sentence. Precision
of all shown points, judged by hand on 100 random corpus sentences: ~88%
(45 wrong of 390), ~96% after the fixes of 2026-11-20 (about 15 of 372). Always take a fresh baseline: these move with every change.

## Open issues

Known, not fixed yet (2026-11-20). Check they still happen before working
on one; remove an entry once fixed.

Segmentation:
- **Noun/verb ties**: a kana noun and a verb or adjective at the same node
  cost, and the noun wins: よいのか as 宵 (not 良い), ゲームをしては as
  仕手 (not する). Needs a cost pass, measured with `score.sh`.
- 置いたらいいか cut as …ら|いい|か wrong (らい|いか); 今なお as な|おそれ;
  やっつけておいで as おい|で; 行くってば as 行|くって.
- 乗り違えた: JMdict has no 乗り違える, so it is 乗り|違えた.

Grammar points (breakdown):
- 中 of 田中さん read as 中 "throughout" (~60 corpus sentences); 間 of
  一時間 as aida-ni. A rule against cuts splitting a two-kanji word lost
  一年中 and 着心地: reverted.
- め of 初めに (me-adjective), だけます of いただけます (kai-dai).
- は〜が across a relative clause (はコンサートが始まるのを…).
- 待たされた is 待たす + passive, not causative-passive: a short causative
  (待たす) and a transitive verb (鳴らす, 動かす, 減らす) look the same,
  so the breakdown can't name it. The answer check could trust the plain
  search there (one verb word covering the text): not done.
- まったく shown as the く adverb (debatable).

Answer check (`Analyzer::check_answer`, `answer_vs_breakdown.rs`):
- Own examples still missed: 楽しかったです (i-adjective-forms), ５００００円
  もした (number-mo; both missed by the plain search too), and the
  segmentation issues above (noka, te-wa, question-ka, nao,
  causative-passive).
- The extension still uses the plain search (`GrammarDict::check_answer`
  in grammar-wasm): bringing the breakdown there means sentence-core in
  a WASM module with JMdict and grammar.bin.
- `ANSWER_SENTENCES` on 200 corpus sentences: the plain search accepts
  ~1300 points the breakdown doesn't find (nearly all wrong); the first
  hybrid 148, about half right; the hybrid trusting the plain search
  inside phrases and longer points 639 (one-char keys: か of かもしれない,
  は of ことは), then 249 with one-char keys only at a phrase's end and
  not inside a longer point (about half right; mi-suffix on 頼み,
  ni-nai-repeat, you-ga among the wrong). Own examples 4682 of 4691: the
  one-char limit lost 机の上に (に of 上に) and 法律上は.

## Next: answer-check precision plan

Started 2026-11-20, to do next session. Hand-judged on 30 fresh corpus
sentences (not used in tuning; `~/.cache/yomeru-sentence/answer-judge/`:
`fresh30.txt`, `fresh30-en.txt`, `judge.txt` with each accepted point per
sentence, B = both checks, N = new only, P = plain only; `ans200.txt` is
the tuning set, keep it out of judging):

| | accepts | right |
|---|---|---|
| plain search | 305 | ~41% |
| `check_answer` | 171 | 74% |
| of which the breakdown's own | 127 | 91% |
| of which plain-search fallback | 44 | 25% |

Steps, each measured with `own_examples` (now 4682 of 4691) and the 30
sentences (`ALL=1 ANSWER_SENTENCES=…/fresh30.txt corpus_accepts`, then
re-judge only what changed); keep a step when wrong accepts drop and own
examples lose little:

Done 2026-11-20 (fresh 30: plain-search accepts 44 → 37, 14 wrong gone,
3 right gained; own examples still 4682):
1. Slot patterns (に〜ない) trusted only when the first piece of a key
   ends a point the breakdown found or sits in a phrase; never for a
   template whose forms are all whole (ni-nai-repeat). Lost: 寄せては返し
   (te-wa-te-wa: 寄せては返す read as one word).
2. A one-char kana key that is a word of its own inside a longer point,
   for a point with no `after` (を of ことを, に of 前に). Kanji keys
   left out: 中 of 庭の中を read as "throughout".
3. Phrase pieces: a key at a phrase's start only when it ends the
   phrase or is its dictionary form (では of ではない, どう of どうしても
   out; 行きつ戻りつ in); a one-char key at its end not for an `amount`
   point (number-wa of には). Stricter tries lost か of 何ですか, と of
   になると.

Still to do:
4. **Done (2026-11-20).** 3+ char patterns inside other words: ものではない read as
   ものだから / もの "because", としている as として, をもて遊ぶ as をもって,
   ませんでした as んだった / たった. `check.rs` `on_edges` with
   `GrammarDict::key_spans`: not cut short by a longer point found from
   the same char, ending on a word edge (or 1 char into the next word,
   2 into a function word, or inside the word it starts), starting on
   one unless after a stem (not on a verb's ん). Both edges strictly
   lost 52 own examples (ていった, うれしそうだった, おかげだった).
   Fresh 30: plain-only accepts 37 → 26 (the 11 removed all wrong); 200
   tuning sentences 249 → 223; own examples 4681 (lost やらせてくれますか:
   a causative point covers せてくれます).
5. **Breakdown errors**, partly done: てくれた no longer 来る's
   potential (`segment::conjugates`), reru-dake no longer on every
   potential (its check is its pattern now). Still open: 時 of ８時 as
   toki, 出かける / 見かける / 話しかける as kakeru (JMdict's glosses
   tell the half-done ones: "start to", "about to"), は私が as は〜が.
   Was (12, also in the Sentence tab): 時 of ８時 as とき
   (after a number: `after`?), potential + reru-dake offered with passive
   on られた and on てくれた (くれる read as 来る's potential), 出かける as
   かける "half-done", ぐるりと / と言われた as と "and", は私が as
   は〜が across a relative clause. These go through the normal workflow
   above (score.sh, compare.sh).
6. **たち/ら inside 彼ら, 私達** (lost vs the plain search): a `suffix`
   flag on tachi would accept ら inside any word: needs a narrower rule
   (the word's entry ends in the key and starts with a pronoun?) or
   leave it.

Then re-judge 30 more fresh sentences (new seed) to confirm, re-run the
200-sentence report, restart the server, and report.
