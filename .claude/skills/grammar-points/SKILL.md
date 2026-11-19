---
name: grammar-points
description: Write Yomeru's own grammar dictionary entries into data/grammar.toml (pattern, meaning, formation, explanation, notes, examples from Tatoeba or our own), fact-checked against bee-san's grammar dictionary without copying it. Trigger when the user asks to write, add, continue, fill, review or fix grammar points or grammar entries, asks for the next batch (e.g. "next N5 batch"), or names grammar patterns to cover.
---

# Grammar points

`data/grammar.toml` is Yomeru's own grammar dictionary: it ships with the
app like JMdict, so **every word of it must be ours** (or Tatoeba's, CC BY).
You write the entries yourself, in this session. Never call the Anthropic
API or ask for an API key.

Bee-san's grammar dictionary (a Yomitan ZIP collecting 14 copyrighted
sources: DoJG, Bunpro, IMABI…) is used two ways only:

- **a checklist**: which grammar points exist, at which level (`bee.py list`);
- **a fact-checker**, *after* an entry is written (`bee.py facts`).

Never take their wording, sentence structure or example sentences, not even
"adapted" or "a variant". A fact you learn from them goes in rewritten from
scratch in your own words. Their text never goes into the repo, not even in
comments or commit messages.

## Files

- `data/grammar.toml`: the entries, checked in. The header comment documents
  every field. Don't read the whole file in a batch: `grep -n 'id = '` or
  look up single entries.
- `examples.utf` (repo root, not committed): the Tanaka Corpus, i.e. Tatoeba
  sentences. Missing? `cargo xtask download-examples`.
- Bee ZIP: kept **outside the repo**, in `~/.cache/yomeru-grammar/bee.zip`
  so later sessions find it (the scratchpad is per session). If it isn't
  there, download it and check it against the publisher's checksums:
  ```bash
  d=~/.cache/yomeru-grammar; u=https://bee-san.github.io/bees-ultimate-grammar-dictionary/downloads
  mkdir -p $d && curl -sfL -o $d/bee.zip $u/bees-ultimate-grammar-dictionary.zip \
    && curl -sfL $u/SHA256SUMS | grep ' bees-ultimate-grammar-dictionary.zip$' \
    | sed "s| bees-ultimate-grammar-dictionary.zip$| $d/bee.zip|" | sha256sum -c -
  ```
- Scripts, in this skill's folder (run with `python3 -I`):
  - `bee.py list ZIP --levels N5 --count 40`: points not covered yet.
    Rows listed in `~/.cache/yomeru-grammar/reviewed.toml` are hidden:
    bee headwords already covered by one of ours under another spelling
    (`"にして①" = "ni-shite"`) or left out on purpose (`"ガン" = "out:
    slang"`). `--reviewed` shows only those, with their note; an unknown
    id prints a warning. After a level's batches, review every row still
    listed: fold real variants into `forms`, put the rest in
    reviewed.toml, and write the real gaps as a small extra batch.
  - `bee.py facts ZIP <pattern>…`: a capped English fact sheet per pattern
    (sources' explanations and caveats, no examples), about 1–2k tokens each.
  - `tatoeba.py '<regex>' --count 10`: example candidates, `<id> <ja> | <en>`.
  - `batch.py status | show N | test [--level n3]`: a level's plan (see
    **Planning a level**): progress per batch; batch N's unwritten points
    with their decisions, example candidates and the `bee.py facts` calls;
    every regex's match count.
  - `examples.py status | show N [--level n5]`: entries still short of 5
    examples (see **An examples batch**): progress per level; batch N with
    each entry's examples, its next `y<n>` key and Tatoeba candidates.
  - `check.py [ZIP]`: validation; with the ZIP, the overlap check.
  - `preview.py OUT.html --bee ZIP`: the whole file as one review page
    (index, search, level filter), each entry linking to bee-san's pages
    for the same point so the user can compare. Write it to the scratchpad and republish the
    review artifact after a batch, so the user reads entries as a learner
    would.

## A batch

About 20 points per batch, **one fresh session per batch**: a long session
re-sends its whole history on every call. Work in this order:

0. **With a plan** (`~/.cache/yomeru-grammar/<level>-plan.toml`, e.g.
   `n4-plan.toml`): run `batch.py show N --level n4` once and skip steps 1 and 3's
   searching. It prints the batch's folds (variants to add to existing
   entries), each point's id, pattern, suggested forms, hover and note, 8
   Tatoeba candidates, and the fact-sheet calls. The decisions are
   suggestions: change them when writing shows a better choice, and update
   the plan file so later batches agree. Run `tatoeba.py` only when no
   candidate fits. Without a plan, start at step 1.
1. **Pick the points.** `bee.py list ZIP --levels N5 --count 40`. Bee names
   some points descriptively (`Verb + てもいい`, `AながらB`) and splits
   variants into rows of their own (てもいいですか): one entry of ours covers
   a point and its variants, through `forms`, and a variant added to `forms`
   drops off the list. A row is hidden only when every pattern it names is
   one of ours exactly, so ながらも still shows after ながら: it is a
   different point. Skip rows that only label a basic structure
   (`Verb + て`) unless there is a real grammar point to explain.
   Bee files some points higher than Bunpro does (でしょう, もらう, the
   particles は, が, も, か, で), so a level isn't finished until the
   higher levels are searched for its Bunpro tag too:
   ```bash
   for l in N4 N3 N2 N1; do python3 -I bee.py list ZIP --levels $l --count 3000; done | grep 'Bunpro N5'
   ```
   The tag comes from matching the pattern's words, so a later point that
   contains the particle (から〜にかけて, 可能形 + だけ) can carry it: write
   the basic use at the Bunpro level, and leave the later point for its own
   level. Bee also has no rows for some basics (に, と, や, possessive の):
   write those at their textbook level and say so in the report.
2. **Write each entry from your own knowledge first**, before any fact
   sheet: meaning, formation, explanation, notes, related.
3. **Examples.** `tatoeba.py` with a regex that catches the conjugated
   forms. Pick 5 short, natural sentences with faithful translations.
   Copy the id, Japanese and English *exactly* (the check compares them with
   the corpus); add only the `{…}` mark. A loose translation ("This is life!"
   for 生きててよかった) disqualifies the sentence: pick another or write
   our own (`source = "yomeru"`, `id = "y1"`, `"y2"`… within the entry).
   Write our own when Tatoeba has nothing simple enough for the level.
4. **Fact-check.** `bee.py facts ZIP <the batch's patterns>`, five patterns
   per call. Look only for facts: a wrong rule, a missing use, a missing
   pitfall or contrast, a register note. Add them in your own words, with
   your own example if one is needed. Don't open more of bee's text than the
   fact sheet.
5. **Disagreements.** When a source says something you believe is wrong,
   don't adopt it: list it for the user. The level is Bunpro's (see
   **jlpt** below); list the points Bunpro doesn't have.
6. **Check.** `python3 -I check.py ZIP` (about 5 s). Rewrite every overlap
   hit, even a coincidence. Short shared runs (a pattern plus a common
   verb, "I make it a rule to") are everyone's and pass; a hit means most of
   a sentence or a long phrase is theirs. The check is blind to short
   phrases, and to IMABI's sentences entirely (it writes furigana inline,
   食はんたべ, so nothing matches): if you notice you borrowed an example
   or a turn of phrase while reading, replace it yourself.
   It also prints `LONG` warnings for the length limits below: trim those.
7. **Review**, before reporting: print the batch's entries compactly
   (pattern, meaning, formation, explanation, notes, examples) and read
   every one as a learner would. The check only covers form; this step
   catches what it can't:
   - **Claims**: is each rule, contrast and "never/only" true? Re-read
     anything written from memory that the fact sheet didn't confirm, and
     any note that contradicts a fact sheet (a source listing a use your
     note rules out).
   - **Level**: example vocabulary and register at the entry's level or
     below (no 至る所, 在宅, いらっしゃいます in an N5 entry); Tatoeba is
     full of hard or literary sentences that match a regex.
   - **Consistency**: an example that contradicts the entry's own notes
     (上げた for "give" when the notes say 上げる is "raise"), contrasts
     that agree across entries, `related` ids that point the right way.
   - **Lookup**: forms that would match inside unrelated words (かった in
     わかった, a bare ら).
   Fix in place, rerun the check, and list in the report what the review
   changed.
8. **Report**: points added per level, ones you're unsure of, the
   disagreements and level differences, what the review fixed, the
   remaining count from `bee.py list`, and suggest the user run `/cost` to
   track the batch's cost.

## An examples batch

Extends written entries to 5 examples, about 20 entries per batch, one
fresh session per batch, level by level from N5.

1. `python3 -I examples.py show N --level n5`: each short entry with its
   examples, the key the next sentence of ours takes, and Tatoeba
   candidates found by its pattern and forms. Too few or too noisy
   candidates: `tatoeba.py` with a regex of the conjugated forms.
2. Add the missing examples to each entry, following **examples** below:
   Tatoeba copied exactly, ours keyed `y<n>`. While there, replace an
   existing example whose English wouldn't lead to the point: remove it
   (its key is never given again) and add another.
3. `python3 -I check.py ZIP`, then the **Review** step of a batch for the
   new sentences: level, naturalness, and each English read as a
   production prompt.
4. **Report**: entries extended, examples replaced and why, `examples.py
   status`.

## Planning a level

One session per level, before its batches, when the user asks for a plan.
Do step 1 for the whole level at once (bee's rows at the level, the Bunpro
tag searched in higher levels, the basics bee has no rows for) and write
`~/.cache/yomeru-grammar/<level>-plan.toml`, outside the repo. Its header
comment lists what was left out and why; then `[[batch]]` tables (`n`,
`title`, `note`, `fold`: variants to add to existing entries) holding
`[[batch.point]]` tables: `id`, `pattern`, `forms`, `hover`, `regex`,
optional `facts` (the `bee.py facts` query when it isn't the pattern),
`jlpt` when it differs, `note` (decisions and open questions). Group
related points in one batch (たら/ば/と/なら, the keigo verbs) and keep
batches near 20. Only our decisions go in, never bee's text. Then run
`batch.py test`: fix any regex that finds nothing, or thousands of noisy
matches. Report the batches, the points left out, and the open questions.

## Writing an entry

Add entries grouped by level (N5, then N4, then N3), each a `[[point]]`
block in the field order of the existing ones. Fix an entry in place;
never add a second block for the same id.

Never Read `data/grammar.toml` whole, so never use Edit on it: Edit needs
a Read first, and the file is thousands of lines. Instead:
- **new entries**: append the batch with one `cat >> data/grammar.toml
  <<'EOF'` heredoc when its level's group is at the end of the file;
  otherwise insert the block with a short Python script after the last
  entry of that level;
- **fixes**: a short `python3 -` script doing exact `str.replace` pairs,
  with `assert s.count(old) == 1` on each so a miss fails loudly. A
  one-line `sed -i` is fine for a single unambiguous line;
- **reading**: `grep -n` for ids, forms and patterns, and `sed -n 'A,Bp'`
  for the few entries you need to compare against.

- **id**: a lowercase romaji slug of the pattern (`te-mo-ii`,
  `wake-de-wa-nai`), with a suffix when two patterns share a spelling
  (`sou-da-appearance`, `sou-da-hearsay`). Links use ids: don't rename
  one without fixing the `related` and `homographs` that name it.
- **seq**: right after `id`. Review cards key on it. A new entry takes the
  file's `next_seq` (`grep -n '^next_seq' data/grammar.toml`), and the batch
  raises `next_seq` past the last number it used, so a batch of 20 at
  `next_seq = 938` gets 938…957 and leaves `next_seq = 958`. Never change
  an entry's seq, and never reuse a removed entry's: a gap is fine.
- **pattern**: the form learners look up. **forms**: other spellings and
  variants the lookup should find (でもいい, ていい, わけじゃない,
  に違いありません). The lookup deinflects, so conjugations it can reach
  (ことにした → ことにする) don't need listing; a form that belongs to
  another entry is an error, except for true homographs (passive and
  potential られる, conditional と and the particle と): both entries keep
  the spelling and name each other in `homographs`, which lets them pass
  the check.
- **jlpt**: Bunpro's level for the point (levels are unofficial since 2010;
  `bee.py list` shows it as `[Bunpro N4]`; bee's own level is the first
  column and can differ, so a list by level can miss a point Bunpro puts
  lower). No Bunpro entry for the point: the usual textbook level, listed
  in the report.
  Bunpro decides even when an entry is already written: no exceptions for
  "taught earlier". But check that the tag's Bunpro card is the same use:
  a bee row can carry the card of another use of the word (the に row's card
  is frequency に, N4; the くらい row's is extent, N3), and that tag doesn't
  move the basic entry.
- **meaning**: one line, at most ~60 chars, English glosses separated by
  commas; a parenthesis for the use when it helps ("(permission)").
- **formation**: one line per construction, in Bunpro's notation: word
  classes `Verb` (bare = plain form), `［い］Adjective`, `［な］Adjective`,
  `Noun`; the form in fullwidth brackets after the word: `Verb［て］`,
  `Verb［た］`, `Verb［る］` (dictionary), `Verb［ない］`, `Verb［stem］`
  (ます-stem), `Verb［できる］` (potential), `［い］Adjective［く］`; a struck
  letter for what's dropped (`［い̶］`, `［な̶い̶］`). Unlike Bunpro, keep the
  grammar in one piece: `Verb［て］ + もいい`, not `+ も + いい`. Several
  classes taking the same ending share a line (`［な］Adjective / Noun +
  でもいい`); a short example in parentheses when the attachment isn't
  obvious. Write each line yourself: the notation is a convention, their
  lines are theirs. Use the same notation in explanations and notes.
- **explanation**: 2–4 sentences, about 80 words at most: what it does,
  when it's used, one tiny in-line example with its translation.
- **notes**: optional, about 80 words at most: pitfalls, register, and the
  look-alikes to tell apart (ようにする vs ようになる vs ことにする), each
  contrast in a few words.
- **related**: ids of patterns worth comparing, written or not yet (the
  check lists the unwritten ones as a to-do).
- **hover**: leave it out (true) unless the pattern would pop up inside
  unrelated words: `hover = false` (って, inside 行って) keeps the entry
  search-only. One-char patterns (は, に, て) need nothing: the popup
  already skips matches under two chars.
- **check**: leave it out when the pattern or a form appears in a correct
  answer (てから, ちゃだめ): production reviews search for them. A point
  that is a verb form gets `check = { inflection = ["causative"] }`
  (passive, potential, causative, causative-passive, volitional,
  imperative). `check = "self"` when neither finds it reliably (the く
  adverb, the て join, 自動詞・他動詞): the user rates against the
  reference. Put it last in the entry, after `examples`.
- **reads**: only for a point that is a word's form rather than a string
  the breakdown can find (the く adverb: `reads = ["adverbial"]`; the さ
  noun: `["nominalization"]`). Names from deinflect's `Reason`. After
  `check`.
- **after**: only for a point the sentence breakdown finds in the wrong
  places, naming what it comes right after: `verb-plain` (走る{な}),
  `masu-stem`, `suru-noun` (に行く after 食べ or 散歩, not 学校), `amount`
  (三十分{は}), `noun-or-demonstrative` (東京{のような}, この{ような}),
  `noun-or-particle` (私{の}, から{の}), `predicate` (a verb or adjective,
  conjugated or not: 行く{の}？, 高い{の}; a form starting with な also
  after a noun: 休み{なの}), `adjective-stem` (寒{がる}, 嫌{がる},
  行きた{がる}). Last.
- **final**: `final = true` for a point that ends a sentence (行く{の}？):
  the breakdown then skips it before more of the sentence (行くのは).
  `continues = true` for one that never ends a sentence (雨{にしろ}…): at
  the end, にしろ reads as にする's command instead. `no_suru = true` for
  a point ending in に that never goes before する/なる (以上に "more
  than"): there the に is にする's (80%以上にしろ). After `after`.
- **examples**: 5 (entries written before 2026-11-18 have 2–3 until an
  examples batch extends them), under ~26 chars of Japanese, at the
  entry's level or below in vocabulary; the `{…}` mark covers the pattern
  as conjugated. Production cards show one at random: the English and the
  `meaning` line, and the learner writes the Japanese. So each English
  must lead to *this* point: "May I borrow this book?" does for てもいい;
  a loose or idiomatic one ("I'd bet my life on it" for 賭けてもいいよ)
  doesn't, even when faithful. Vary the sentences: different verbs,
  subjects and the entry's main forms (statement, question, negative),
  not five rewordings of one.
- **example keys**: reports and verdicts point at an example by its key.
  A Tatoeba example is keyed by its `tatoeba` pair; one of ours carries
  `id = "y<n>"`, unique within the entry: a new one takes the next number
  up (`examples.py show` prints it), and a removed one's number is never
  given again. Never renumber.
- **also**: optional on an example, `also = ["間に合ってよかった。"]`:
  other answers to its English a learner would rightly write (a word the
  English doesn't call for left out, another word order), plain, no
  `{…}`. The review diffs an answer against the closest of the sentence
  and these. Don't list mere politeness variants (だ/です, る/ます).
- Plain English for learners: no linguistics jargon (連用形, ergative),
  no "we will learn", no quotes from sources.
