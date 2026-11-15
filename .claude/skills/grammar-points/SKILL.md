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
  - `bee.py facts ZIP <pattern>…`: a capped English fact sheet per pattern
    (sources' explanations and caveats, no examples), about 1–2k tokens each.
  - `tatoeba.py '<regex>' --count 10`: example candidates, `<id> <ja> | <en>`.
  - `check.py [ZIP]`: validation; with the ZIP, the overlap check.
  - `preview.py OUT.html --bee ZIP`: the whole file as one review page
    (index, search, level filter), each entry linking to bee-san's pages
    for the same point so the user can compare. Write it to the scratchpad and republish the
    review artifact after a batch, so the user reads entries as a learner
    would.

## A batch

About 20 points per batch, **one fresh session per batch**: a long session
re-sends its whole history on every call. Work in this order:

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
   forms. Pick 2–3 short, natural sentences with faithful translations.
   Copy the id, Japanese and English *exactly* (the check compares them with
   the corpus); add only the `{…}` mark. A loose translation ("This is life!"
   for 生きててよかった) disqualifies the sentence: pick another or write
   our own (`source = "yomeru"`). Write our own when Tatoeba has nothing
   simple enough for the level.
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

## Writing an entry

Add entries with Edit, grouped by level (N5, then N4, then N3), each a
`[[point]]` block in the field order of the existing ones. Fix an entry in
place; never add a second block for the same id.

- **id**: a lowercase romaji slug of the pattern (`te-mo-ii`,
  `wake-de-wa-nai`), with a suffix when two patterns share a spelling
  (`sou-da-appearance`, `sou-da-hearsay`). Cards point at ids: never rename
  one once committed.
- **pattern**: the form learners look up. **forms**: other spellings and
  variants the lookup should find (でもいい, ていい, わけじゃない,
  に違いありません). The lookup deinflects, so conjugations it can reach
  (ことにした → ことにする) don't need listing; a form that belongs to
  another entry is an error.
- **jlpt**: Bunpro's level for the point (levels are unofficial since 2010;
  `bee.py list` shows it as `[Bunpro N4]`; bee's own level is the first
  column and can differ, so a list by level can miss a point Bunpro puts
  lower). No Bunpro entry for the point: the usual textbook level, listed
  in the report.
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
- **examples**: 2–3, under ~26 chars of Japanese, at the entry's level or
  below in vocabulary; the `{…}` mark covers the pattern as conjugated.
- Plain English for learners: no linguistics jargon (連用形, ergative),
  no "we will learn", no quotes from sources.
