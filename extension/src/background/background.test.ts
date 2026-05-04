import { describe, it, expect } from "vitest";
import { mergeReview } from "./review-utils.ts";
import type { SrsCard } from "../shared/types.ts";

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  return {
    word: "食べる",
    reading: "たべる",
    meaning_en: "to eat",
    due_ms: 0,
    interval: 1,
    ease: 2.5,
    reps: 0,
    added_ms: 0,
    ...overrides,
  };
}

describe("mergeReview", () => {
  it("preserves senses from the original card", () => {
    const senses = [{ pos: ["v1"], glosses: [{ text: "to eat" }] }];
    const original = makeCard({ senses });
    const reviewed = makeCard({ due_ms: 86_400_000, interval: 2, senses: undefined });

    const result = mergeReview(original, reviewed);

    expect(result.senses).toBe(senses);
  });

  it("carries over updated scheduling fields from the reviewed card", () => {
    const original = makeCard({ due_ms: 0, interval: 1, ease: 2.5, reps: 0 });
    const reviewed = makeCard({ due_ms: 86_400_000, interval: 4, ease: 2.6, reps: 1 });

    const result = mergeReview(original, reviewed);

    expect(result.due_ms).toBe(86_400_000);
    expect(result.interval).toBe(4);
    expect(result.ease).toBe(2.6);
    expect(result.reps).toBe(1);
  });

  it("preserves undefined senses when original card has none", () => {
    const original = makeCard({ senses: undefined });
    const reviewed = makeCard({ due_ms: 86_400_000 });

    const result = mergeReview(original, reviewed);

    expect(result.senses).toBeUndefined();
  });
});
