---
name: kanji-mnemonics
description: Write kanji mnemonic phrases into data/kanji-mnemonics.tsv (the mnemonic line on kanji cards) and fix plain part names in data/part-names.tsv. Trigger when the user asks to fill, add, continue, write, rewrite or review mnemonics or phrases for kanji, asks for a phrase for a specific kanji, says a kanji has no phrase, or wants the kanji of their deck covered.
---

# Kanji mnemonics

Each kanji card shows a short memory phrase that links its parts' meanings to the kanji's meaning, e.g. `惑 A heart torn between this or that: perplexed.` You write these phrases yourself, in this session. Never call the Anthropic API or ask for an API key: there is deliberately no API-based generator in this repo.

## Files

- `data/kanji-mnemonics.tsv`: `<kanji><TAB><phrase>` per line, `#` comments. This is the source of truth, checked in.
- `data/part-names.tsv`: plain names for parts (`宀<TAB>roof`). The chips show these first and the helper uses them first.
- Both are laid over KANJIDIC at lookup time (`kanjidic-core`: `init_mnemonics`, `init_part_names`). Never put them into `kanjidic.bin`.
- `cargo xtask build --profile dev` (or `build-dicts` / `build-all`) copies both into `extension/data/`. A running server only reads them at startup, so restart it to see changes.

## Workflow

1. **Get the entries dump** (skip if it already exists in the scratchpad):
   ```bash
   cargo run -q -p kanjidic-build --release -- --input kanjidic2 --kanjivg kanjivg.xml \
       --output <scratchpad>/kanjidic.bin --dump-json <scratchpad>/kanji.json
   ```
   `kanjidic2` and `kanjivg.xml` are at the repo root. If they're missing, run `cargo xtask download-kanjidic`; `build-kanjidic` downloads KanjiVG.

2. **List what to write**, about 150 kanji per chunk:
   ```bash
   python3 -I .claude/skills/kanji-mnemonics/next_kanji.py <scratchpad>/kanji.json --count 150
   ```
   - `--only 慣論` for specific kanji (e.g. one the user says has no phrase).
   - `--deck <backup.json>` puts the kanji cards of an app backup export first, soonest due first. Prefer this when the user has given a backup: their own cards matter most.
   - Otherwise the order is Jōyō + JLPT kanji, most frequent first. Kanji that already have a phrase are skipped.

   Each line reads `kanji [meanings] part=meaning/meaning + part=…`. The part meanings are exactly what the review chips show.

3. **Write the phrases** and append them with a heredoc (`cat >> data/kanji-mnemonics.tsv <<'EOF'`). Use a real tab between kanji and phrase. To change an existing line, rewrite it in place (a short Python edit) and never add a second line for the same kanji.

4. **Validate** (format, duplicates, length):
   ```bash
   python3 -I .claude/skills/kanji-mnemonics/next_kanji.py <scratchpad>/kanji.json --check
   ```

5. **Copy and report:** run `cargo xtask build --profile dev`. Tell the user how many phrases were added, which ones you're unsure of, and how many in-scope kanji remain (the helper prints this on stderr).

## Writing a phrase

- **One line, at most 15 words**, ending on the kanji's main meaning, after a colon when it reads well: `A person leaning against a tree: rest.`
- **Build it from the part meanings as listed**, so the learner sees the chip words in the phrase. Pick whichever meaning makes it click, not necessarily the first: 或 is "some", but "or" gives 惑.
- **Concrete scenes beat abstract wording.** A small action (grabs, leans, cuts, pierces) beats "X and Y together".
- **Let the scene explain the meaning**, not just put the parts side by side: give the reason the parts lead to it. `質 Shellfish so fine it takes two axes to crack them: quality.` works because the quality is *why* two axes are needed; "Two axes weighed against shellfish: quality" only lists the parts. The reason can be invented; it doesn't have to follow etymology.
- **Parts that are only a sound hint** can still be used for their meaning, or left out when they add nothing.
- **Kanji with no parts:** describe the shape (`A crescent moon with two lines of shadow: moon.`) or a gesture.
- **No puns on Japanese readings**, no quotes around the phrase, no tabs or line breaks inside it.
- **If a phrase is weak** (abstract parts such as 九 or 当), still write the best one you can, and list it as weak in your report so the user can rewrite it.

## Part names

A part label that is jargon ("dotted cliff", "katakana u"), a unit ("8 1/3lbs"), or `?` makes a bad chip and a bad phrase. Add a plain name to `data/part-names.tsv` (`广<TAB>shelter`), then rerun the listing so the new name is used.

When you add or change a part name, find the existing phrases that used the old label, rewrite them, and tell the user which part names you added.
