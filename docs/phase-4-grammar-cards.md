# Phase 4 — grammar points as review cards

Goal: add a grammar point to the deck from the Lookup page or the hover
popup, and review it with FSRS like words and kanji, on every device.

## What the dictionary is now

The grammar points come from Yomeru's own `data/grammar.toml`, compiled
into `grammar.bin` and shipped with every build like JMdict (server,
Android assets, extension `data/`). Every device has the same entries, so a
grammar card draws its faces from the shipped dictionary the way word
cards draw theirs from JMdict. The card still syncs only a pointer and its
SRS state: the content is in every build already.

## 1. A stable ID per grammar point

Each entry has an `id` slug (`te-mo-ii`) that never changes (the TOML's
header says so). `grammar.bin` keeps an id → entry table, and
`GrammarDict::by_id` reads it. **Done.**

## 2. The card

- New `CardDirection::Grammar` (wire string `"grammar"`). The server stores
  `direction` as free text, so **no server schema change**.
- `sequence` = `grammar_seq(slug)` = `0x8000_0000 | (fnv1a_32(slug) &
  0x7FFF_FFFF)`, the way kanji cards use the code point. The range sits far
  above JMdict `ent_seq` (< 10M) and kanji (< 0x40000), so the card `id`
  stays `card_id(sequence, direction)` (`"<seq>::grammar"`) and the stores,
  the `sequence` index and the server column need no change. The hash is
  fixed forever. `compile` fails if two slugs collide, and
  `GrammarDict::by_seq` resolves a card back to its entry.
- Code reading `sequence` as a JMdict entry must skip grammar cards:
  `get_cards_by_sequence` users, the page highlighter's sequence list,
  trouble words, the word list, the kanji deck reconciliation
  (`kanji_deck.rs`), stats, export/import (`cards_io.rs`). A card is a word
  card when `is_word()` (neither kanji nor grammar).
- What reaches the server: the number and the SRS fields.

## 3. Faces

Start with one direction, add the second once the first feels right:

1. **Recognition** (first): front = the pattern (term, reading, structure);
   back = meaning, level, formation, two examples, and "Open full
   explanation" to the Lookup page.
2. **Cloze** (later): front = an example sentence with its highlighted span
   blanked, plus its translation; back = the pattern. The examples already
   mark the pattern as `Highlight` spans, so the blank is cheap to make.

## 4. A point that left the dictionary

The dictionary ships with the app, so every device has it. What can happen
is an entry removed or merged in a later `grammar.toml`: `by_id` then finds
nothing. Such cards stay out of the review session and keep their
schedule; ids are never reused, so nothing else can match them.

## 5. Adding cards

- Lookup page: "Add to deck" on `GrammarEntryCard` (and the state: in deck /
  staging / graduated, like words).
- Extension popup: the same button on the Grammar tab, through a new
  `ADD_GRAMMAR { id }` message to the background, and a grammar status in
  `GET_WORD_STATUS` (or a sibling message keyed by ID).
- Same staging → active flow as words, so new grammar points enter through
  the New tab at the user's pace.

## 6. Review

- Proposal: a **Grammar** review section beside Words and Kanji (as kanji
  got its own), so word statistics and the trouble list stay about words.
- FSRS: shared parameters for now. Per-type weights wait for enough reviews
  (see the deferred optimizer note); the review log already records
  `card_id`, so they can be split later without migrating anything.

## 7. Order of work

1. ~~grammar-core: `id`, format v3, `by_id`, tests~~ (done with the
   switch to `data/grammar.toml`). Then `grammar_seq`, the collision check
   in `compile`, `by_seq`, exposed in `grammar-wasm`.
2. yomeru-core: `CardDirection::Grammar`, card constructor, the
   sequence-keyed code paths skipping grammar cards, export/import.
   Unit tests for each skip.
3. Sync: round-trip a grammar card through the server integration tests
   (the wire fixture gains one card) — proves only the ID travels.
4. UI: Add button on the Lookup page; Grammar review section with the
   recognition faces.
5. Extension: `ADD_GRAMMAR`, status, button on the popup's Grammar tab.
6. Later: production reviews (below), then maybe cloze.

## Decisions

1. Grammar review has its own section, beside Words and Kanji.
2. Recognition first. Production (English → Japanese) comes next.
3. Cards whose point was removed from `grammar.toml` are hidden and keep
   their schedule.

## Later: production reviews

The review that trains grammar best: an English sentence, the user gives
the Japanese. Four parts, in order.

### 1. More examples per point

One card shows a different example each time, so the sentence can't be
learned by heart. Before writing examples at scale, the format grows:

- A stable key per example: the `tatoeba` pair, or an `id` on `yomeru`
  examples. Reports and verdicts point at it.
- Vocabulary hints are generated from JMdict (`find_in_text` on `ja` with
  the `{…}` span left out), plus an optional hand override for the cases
  where the automatic hint is wrong.

### 2. The production face

- Front: the English sentence, and under it the point's `meaning` line
  ("prohibition: must not"), so the user knows which point to use without
  seeing the pattern. The vocabulary hints are behind a button.
- Back: the reference sentence with the pattern highlighted.

### 3. Validation

- The answer is checked in two parts:
  - automatic: does the target point appear? Run `GrammarDict::hover_match`
    with the point's `forms` over the answer, so ちゃだめ and
    てはいけません both count;
  - self-graded: is the rest right? The user compares with the reference
    sentence and rates.
- When the check says "point not found" but the user thinks the sentence
  is valid (another point that fits the translation, say), a "My answer is
  valid" button sends a report `{point, example key, answer, time}` to the
  server. Only on that button, never on its own.
- Open question: typed answer (better for learning, and needed for the
  automatic check) or answered in the head and self-graded (fast on a
  phone). Proposal: typed by default, with a setting "answer in my head"
  that skips the check.

### 4. Refining the examples with notes

- `cargo xtask grammar-reports pull` writes the open reports to a TOML
  file. Claude evaluates them in session (no API) and writes the verdicts
  on the example in `grammar.toml`:

  ```toml
  { tatoeba = "69322_231955", ja = "…", en = "…",
    answers = [ { ja = "私のペンを使っちゃだめ", ok = true },
                { ja = "私のペンを使わないで", ok = false,
                  note = "ないで is a request, not a rule" } ] }
  ```

  `cargo xtask grammar-reports resolve` then closes the reports on the
  server.
- The verdicts ship with the dictionary, so every user sees the note, not
  only the one who reported. A normalized answer is compared with
  `answers` before the automatic check: an accepted answer passes, a
  rejected one shows its note.
