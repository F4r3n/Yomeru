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

- Each entry carries its own number, `seq`, issued from a counter in the
  TOML header that only goes up:

  ```toml
  next_seq = 938

  [[point]]
  id  = "te-mo-ii"
  seq = 417
  ```

  A new entry takes `next_seq` and raises it by one. `compile` fails if two
  entries share a `seq` or one is not below `next_seq`.
- The number, not the slug, keys the card: a grammar card is
  `kind = "grammar"`, `item = "417"` (see [Card keys](#card-keys)). A
  removed entry's number is never issued again, because the counter never
  goes down. A slug that comes back later gets a new number, so the old
  cards don't attach to it. No hash, no collisions, no list of retired
  ids. The slug stays the readable key for `by_id` and links;
  `GrammarDict::by_seq` resolves a card back to its entry.
- Why not a hash of the slug (the first version, `bbe2020b`): a reused
  slug gives the same number, and `compile` can only check collisions
  among the entries still in the file.
- What reaches the server: the key and the SRS fields.

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
is an entry removed or merged in a later `grammar.toml`: `by_seq` then finds
nothing. Such cards stay out of the review session and keep their
schedule; the counter never issues their number again, so nothing else can
match them.

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
   switch to `data/grammar.toml`). ~~Then `grammar_seq`, the collision
   check in `compile`, `by_seq`, exposed in `grammar-wasm`.~~
2. ~~yomeru-core: `CardDirection::Grammar`, card constructor, the
   sequence-keyed code paths skipping grammar cards, export/import.
   Unit tests for each skip.~~
3. ~~Sync: round-trip a grammar card through the server integration tests
   (the wire fixture gains one card) — proves only the ID travels.~~ Also
   added the `grammar_cards` flag and the `~nogrammar` epoch, removed
   again by the card key migration.
4. ~~UI: Add button on the Lookup page; Grammar review section with the
   recognition faces.~~
5. ~~Extension: `ADD_GRAMMAR`, status, button on the popup's Grammar tab.~~
6. ~~The card key migration ([Card keys](#card-keys)), which also
   replaces `grammar_seq` with the `seq` counter and removes the
   `grammar_cards` flag.~~ Rolled out by hand: see [Rollout](#rollout).
7. Later: production reviews (below), then maybe cloze.

Gap: the Stats page has Words and Kanji tabs only; grammar cards show in
no statistics yet.

## Decisions

1. Grammar review has its own section, beside Words and Kanji.
2. Recognition first. Production (English → Japanese) comes next.
3. Cards whose point was removed from `grammar.toml` are hidden and keep
   their schedule.
4. Cards are keyed by `kind` + `item` + `direction`, not by a `u32`
   split into value ranges ([Card keys](#card-keys)).
5. Clients keep cards of a kind or direction they don't know, unseen, so a
   new card type needs no server change and no sync flag.
6. A grammar card's `item` is the entry's `seq`, issued from the
   `next_seq` counter in `grammar.toml`, not a hash of its slug.
7. The migration wipes the clients' card stores and pulls everything from
   the migrated server; only the server's data is converted.
8. Production answers are checked for the point only, in three cases:
   accepted, close (another point from `related`: free retry), too far
   (hints in steps). See [Validation](#3-validation).
9. A hint caps the rating at Hard; seeing the reference means Again.
10. Typed answers by default; "answer in my head" is a local setting.
11. Points that are a verb form are checked by deinflection
    (`check = { inflection = [...] }`); those neither finds reliably are
    self-graded (`check = "self"`).
12. Production examples: 5 per point, keyed by `tatoeba` pair or `y<n>`;
    vocabulary hints computed at runtime.

## Card keys

Grammar cards aren't released (neither the apps nor the server) and the
only clients are the author's devices, so the card key can be redone from
scratch, with one migration and a backup.

### Before and after

| | Before | After |
|---|---|---|
| Key | `sequence: u32`, kind told apart by range (`ent_seq`, code point, top bit) | `kind` + `item` (text) + `direction` |
| Card id | `"1358280::recognition"`, `"28450::kanji"`, `"2147484065::grammar"` | `"word:1358280:recognition"`, `"kanji:漢:meaning"`, `"grammar:417:recognition"` |
| Directions | one flat enum | per kind: word {`recognition`, `recall`}, kanji {`meaning`}, grammar {`recognition`, later `production`} |
| Unknown type | fails the whole sync response | parsed card by card; unknown kinds are kept, never shown |
| Grammar `item` | FNV-1a hash of the slug | `seq` from the counter |

Goes away: `grammar_seq` and its range, `is_grammar_seq`, range checks
behind `is_word()`, `sibling_ids` guessing the kind from the number, the
`grammar_cards` request flag and the `~nogrammar` epoch.

### Migration

- **Server**, once, in `init_schema`, when `cards` still has a `sequence`
  column: rewrite `cards` (`kind`, `item`, new `id`) from `sequence` and
  `direction`; rewrite `reviews.card_id` the same way (the review log is
  the FSRS history, and the server holds its only full copy); clear
  `deletions`; give `sync_meta` a new epoch so every client gets a full
  answer. A second start changes nothing.
- **Clients** (IndexedDB v10 in the app and the extension, a new
  `user_version` in the app's SQLite store): no conversion. The upgrade
  clears cards, tombstones, review outbox, drill counts, dirty and the
  sync cursor, keeps settings and lookup history, and the next sync pulls
  everything. Unsynced local changes are lost: hence the rollout order.
- **Extension auto-backup** (`storage.local`, `cards-backup.ts`): when the
  store is empty and a snapshot exists, `syncCardsBackup` restores it. After
  the wipe it would restore old-format cards, so the snapshot moves to a new
  key and the old one is removed, never restored. A card without `kind` and
  `item` is never restored either.
- **Export**: schema 3. Import still reads schema 2 (today's files) and
  converts it, so the backups taken before the migration import again.

### Tests first (step 0)

Written against today's code, so they pin what the backups contain before
anything changes:

1. `testdata/backup/export-v2.json`: a small export written by today's
   export code (words in both directions, a kanji, a staged and a graduated
   card, reviewed fields). Today it imports field for field; after the
   change it imports through the v2 → v3 conversion with the expected ids.
2. Server: a database with today's schema (cards of every kind, reviews,
   deletions). After the migration every card is rewritten, the reviews per
   card are unchanged under the new ids, deletions are empty, the epoch is
   new, and a second start is a no-op.
3. Extension: an old-format `storage.local` snapshot is never restored as
   is into the new store.
4. Ignored, on the real files (like `real_dict`):
   `YOMERU_DB=copy.db cargo test -p server real_db -- --ignored --nocapture`
   migrates a copy of the server database and checks every card and answer
   under its new id; `YOMERU_EXPORT=deck.json cargo test -p yomeru-core
   real_export -- --ignored` imports a real export. Both files stay
   outside the repo.

### Rollout

1. Export the deck on each device; copy the server's SQLite file.
2. Sync every device, so nothing is left in an outbox.
3. Run test 4 on the copies. Deploy the server, then check cards and
   reviews per account against the copy.
4. Update each device: it wipes its cards and pulls them back.
5. Compare counts with step 1.

## Later: production reviews

The review that trains grammar best: an English sentence, the user gives
the Japanese. Four parts, in order.

### 1. More examples per point

One card shows a different example each time, so the sentence can't be
learned by heart. Before writing examples at scale, the format grows:

- ~~A stable key per example: the `tatoeba` pair, or an `id` on `yomeru`
  examples. Reports and verdicts point at it.~~ Done: `id = "y<n>"`,
  unique within the entry and never given again; `Example::key`,
  `grammar.bin` format 5.
- Vocabulary hints are generated from JMdict at runtime, when the user
  taps for them (`find_in_text` on `ja` with the `{…}` span left out), so
  `grammar.bin` and its build don't need JMdict. An optional
  `hints = […]` on the example overrides them, added only once a
  generated hint turns out wrong.
- 5 examples per point, written with the skill's examples batches
  (`examples.py`). N5 done (2026-11-18); N4–N1 to follow. Each English
  must lead to the point, but variants are the check's job (below), not
  the sentences'.

### 2. The production face

- Its own card: direction `production`, id `"grammar:<seq>:production"`,
  beside the recognition card on the same `item`. Older clients keep it
  unseen until they update.
- Code that assumes one card per point takes both, as words do:
  promote / demote / reset / delete and the Add button's status.
  ~~Done~~ (branch `grammar-production`): `CardItem::directions`, the
  extension's `KIND_DIRECTIONS`; Add makes both; the button shows the
  recognition card.
- Points added before: `ensure_grammar_production_cards` (on the Grammar
  review and New Words screens) stages the missing production card,
  written as synced and stamped with the recognition card's `added_ms`, so
  another device's real copy always wins the merge.
- Sessions: the Grammar tab's start screen picks a mode, Mix,
  Recognition or Production (`GrammarMode`, remembered on the device),
  each with its due count (`count_due_grammar`). A session takes one card
  per item (`srs::hold_siblings`); the other stays due for the next
  session. This holds for a word's recognition and recall cards too, in
  the app and the extension.
- ~~Done~~ (branch `grammar-production`): the face below, the check on
  every platform (`DictClient::grammar_check`: local on Android, `POST
  /api/grammar-check` for the web, grammar-wasm calling back into
  jmdict-wasm in the extension), the hint, the caps, "answer in my head"
  on the start screen. A "My answer was right" link on the back lifts the
  Again cap when the check missed a valid answer; it sends no report yet
  (part 4).
- Front: the English sentence, and under it the point's `meaning` line
  ("prohibition: must not"), so the user knows which point to use without
  seeing the pattern. The vocabulary hints are behind a button.
- Back: the reference sentence with the pattern highlighted.

### 3. Validation

Many answers can be right for one prompt: だ or です, plain or polite,
行きます or 来ます, another word order. The check only asks whether the
answer uses the card's point, sorts the answer into one of three cases,
and leaves the rest to the user.

1. **The point is there: accepted.** `GrammarDict::hover_match` over the
   answer, with the point's pattern and `forms` and the popup's
   deinflection, so だ and です both count for `desu`, ちゃだめ and
   てはいけません both count for prohibition. The rest isn't checked: the
   reference sentence is shown beside the answer and the user rates, as on
   word cards.
2. **Another point is there.** The same search over the whole grammar
   dictionary, keeping matches of two characters or more (the popup's
   rule, so に, は, を don't count):
   - a point in the card's `related` list: **close**. "たあとで works here
     too, but this card practises てから." A free retry: it doesn't count
     against the grade;
   - any other point: no nudge, the answer goes to case 3, but the hints
     name it: "You used てください. This card asks for てから."
   `related` lists the points that do the same job, so only those earn
   "works here too". An alternative missing from it comes back through
   the reports (below) and can be added.
   Each entry says how case 1 finds it, in `check` (`AnswerCheck`):
   - left out: the pattern and `forms` are searched (most points). A
     leading る stands for the dictionary form (ることがある, るところだ,
     るばかりだ): it matches any dictionary-form ending, so 行くことがある
     counts. The hover popup has the same gap (see
     `grammar-open-questions.md`);
   - `{ inflection = [...] }`: a point that is a verb form (passive,
     potential, causative, causative-passive, volitional, imperative). A
     verb of the answer must deinflect through one of them, so
     食べさせた counts for the causative. ~~The deinflector returns the
     rules' reasons and has the volitional~~ (done:
     `deinflect_with_reasons`, a `Reasons` bitset per form, joined over
     every path), mapped onto `Inflection` by
     `GrammarDict::check_answer` (grammar-core `answer.rs`), which takes a
     `Vocabulary` (JMdict in the apps) for "is this base a verb" and the
     word boundaries a one-char kana point needs. The ichidan られる
     is both passive and potential and counts for either card;
   - `"self"`: none of this finds it reliably (the く adverb, the て join,
     自動詞・他動詞). No check and none of the three cases: the user rates
     against the reference, as in "answer in my head".
3. **Too far: hints in steps.** Each tap shows one more: the `meaning`
   line (already on the front), the formation (`Verb［て］ + から`), the
   vocabulary hints, then the reference sentence.

Grading:
- No hint: the user's own rating.
- Any hint (formation or vocabulary): the rating is capped at Hard.
- The reference shown before a correct answer: Again.
- When the check misjudges (a valid sentence it doesn't recognise, or an
  alternative case 2 should have accepted), a "My answer is valid" button
  sends a report `{point, example key, answer, time}` to the server. Only
  on that button, never on its own.
- Typed answers by default: better for learning, and the check needs
  them. A local setting (per device, not synced) switches to "answer in
  my head": the user thinks of the sentence, reveals the reference and
  rates, with no check and none of the three cases. Typing suits a
  keyboard, the head a phone, so each device keeps its own choice.

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
