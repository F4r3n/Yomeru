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
6. Next, before grammar cards ship in a release: the card key migration
   ([Card keys](#card-keys)), which also replaces `grammar_seq` with the
   `seq` counter and removes the `grammar_cards` flag.
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

## Card keys

Grammar cards aren't released (neither the apps nor the server) and the
only clients are the author's devices, so the card key can be redone from
scratch, with one migration and a backup.

### Before and after

| | Today | After |
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
  the wipe it would restore old-format cards, so the upgrade converts the
  snapshot or clears it.
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
   `YOMERU_DB=copy.sqlite YOMERU_EXPORT=deck.json cargo test --ignored`
   migrates a copy of the server database, imports a real export, and
   compares counts. Both files stay outside the repo.

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

- A stable key per example: the `tatoeba` pair, or an `id` on `yomeru`
  examples. Reports and verdicts point at it.
- Vocabulary hints are generated from JMdict (`find_in_text` on `ja` with
  the `{…}` span left out), plus an optional hand override for the cases
  where the automatic hint is wrong.

### 2. The production face

- Its own card: direction `production`, id `"grammar:<seq>:production"`,
  beside the recognition card on the same `item`. Older clients keep it
  unseen until they update.
- Code that assumes one card per point takes both, as words do:
  promote / demote / reset / delete and the Add button's status.
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
