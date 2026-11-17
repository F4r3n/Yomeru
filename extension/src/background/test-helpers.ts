// Shared test fixtures for the background module's test suites
// (background.test.ts, cards-backup.test.ts, idb.test.ts).

import type { SrsCard } from "../shared/types.ts";
import { cardId, wordKey } from "../shared/types.ts";

/** A card; a word's (`sequence`, default 1358280) unless `kind`/`item` say
 * otherwise. */
export function makeCard(
  overrides: Partial<SrsCard> & { sequence?: number } = {},
): SrsCard {
  const { sequence, ...rest } = overrides;
  const word = wordKey(sequence ?? 1_358_280);
  const key = { kind: rest.kind ?? word.kind, item: rest.item ?? word.item };
  const direction = rest.direction ?? (key.kind === "kanji" ? "meaning" : "recognition");
  return {
    id: cardId(key, direction),
    ...key,
    direction,
    due_ms: 0,
    stability: 0,
    difficulty: 0,
    reps: 0,
    lapses: 0,
    state: "new",
    last_review_ms: null,
    added_ms: 0,
    status: "active",
    priority: 0,
    ...rest,
  };
}
