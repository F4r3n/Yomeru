import { describe, it, expect } from "vitest";
import { mergeReview, applyIntervalScale, checkGraduation } from "./review-utils.ts";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  const word = overrides.word ?? "食べる";
  const direction = overrides.direction ?? "recognition";
  return {
    id: cardId(word, direction),
    word,
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
    ...overrides,
  };
}

describe("mergeReview", () => {
  it("carries over updated FSRS scheduling fields from the reviewed card", () => {
    const original = makeCard({ due_ms: 0, stability: 0, difficulty: 0, reps: 0, state: "new" });
    const reviewed = makeCard({
      due_ms: 86_400_000,
      stability: 4.2,
      difficulty: 5.5,
      reps: 1,
      lapses: 0,
      state: "review",
      last_review_ms: 0,
    });

    const result = mergeReview(original, reviewed);

    expect(result.due_ms).toBe(86_400_000);
    expect(result.stability).toBe(4.2);
    expect(result.difficulty).toBe(5.5);
    expect(result.reps).toBe(1);
    expect(result.state).toBe("review");
    expect(result.last_review_ms).toBe(0);
  });

  it("forces status to active regardless of input", () => {
    const original = makeCard({ status: "staging" });
    const reviewed = makeCard({ status: "staging" });

    const result = mergeReview(original, reviewed);

    expect(result.status).toBe("active");
  });
});

describe("applyIntervalScale", () => {
  it("scales stability and recomputes due_ms from the remaining interval", () => {
    const nowMs = 1_000_000_000;
    const card = makeCard({ stability: 4, due_ms: nowMs + 4 * 86_400_000 });

    const result = applyIntervalScale(card, 1.5, nowMs);

    expect(result.stability).toBe(6);
    expect(result.due_ms).toBe(nowMs + 6 * 86_400_000);
  });

  it("returns the same card reference when scale is 1.0", () => {
    const card = makeCard({ stability: 4, due_ms: 123 });

    expect(applyIntervalScale(card, 1.0, 0)).toBe(card);
  });

  it("does not mutate the original card", () => {
    const card = makeCard({ stability: 4 });

    applyIntervalScale(card, 2.0, 0);

    expect(card.stability).toBe(4);
  });

  it("handles sub-1 scale (faster review)", () => {
    const nowMs = 0;
    const card = makeCard({ stability: 10, due_ms: 10 * 86_400_000 });

    const result = applyIntervalScale(card, 0.5, nowMs);

    expect(result.stability).toBe(5);
    expect(result.due_ms).toBe(5 * 86_400_000);
  });
});

describe("checkGraduation", () => {
  it("returns true when reps meet the threshold", () => {
    expect(checkGraduation(5, 5)).toBe(true);
  });

  it("returns true when reps exceed the threshold", () => {
    expect(checkGraduation(7, 5)).toBe(true);
  });

  it("returns false when reps are below the threshold", () => {
    expect(checkGraduation(4, 5)).toBe(false);
  });

  it("returns false when graduationReps is 0 (graduation disabled)", () => {
    expect(checkGraduation(100, 0)).toBe(false);
  });
});
