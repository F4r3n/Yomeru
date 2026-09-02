// Shared test fixtures for the background module's test suites
// (background.test.ts, cards-backup.test.ts, idb.test.ts).

import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

export function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  const sequence = overrides.sequence ?? 1_358_280;
  const direction = overrides.direction ?? "recognition";
  return {
    id: cardId(sequence, direction),
    sequence,
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
    consecutiveCorrect: 0,
    ...overrides,
  };
}
