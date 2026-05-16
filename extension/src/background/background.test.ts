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
    interval_days: 1,
    ease_factor: 2.5,
    repetitions: 0,
    added_ms: 0,
    status: "active",
    ...overrides,
  };
}

describe("mergeReview", () => {
  it("carries over updated scheduling fields from the reviewed card", () => {
    const original = makeCard({ due_ms: 0, interval_days: 1, ease_factor: 2.5, repetitions: 0 });
    const reviewed = makeCard({ due_ms: 86_400_000, interval_days: 4, ease_factor: 2.6, repetitions: 1 });

    const result = mergeReview(original, reviewed);

    expect(result.due_ms).toBe(86_400_000);
    expect(result.interval_days).toBe(4);
    expect(result.ease_factor).toBe(2.6);
    expect(result.repetitions).toBe(1);
  });

  it("forces status to active regardless of input", () => {
    const original = makeCard({ status: "staging" });
    const reviewed = makeCard({ status: "staging" });

    const result = mergeReview(original, reviewed);

    expect(result.status).toBe("active");
  });
});

describe("applyIntervalScale", () => {
  it("scales interval_days and recomputes due_ms", () => {
    const nowMs = 1_000_000_000;
    const card = makeCard({ interval_days: 4, due_ms: 0 });

    const result = applyIntervalScale(card, 1.5, nowMs);

    expect(result.interval_days).toBe(6);
    expect(result.due_ms).toBe(nowMs + 6 * 86_400_000);
  });

  it("returns the same card reference when scale is 1.0", () => {
    const card = makeCard({ interval_days: 4, due_ms: 123 });

    expect(applyIntervalScale(card, 1.0, 0)).toBe(card);
  });

  it("does not mutate the original card", () => {
    const card = makeCard({ interval_days: 4 });

    applyIntervalScale(card, 2.0, 0);

    expect(card.interval_days).toBe(4);
  });

  it("handles sub-1 scale (faster review)", () => {
    const nowMs = 0;
    const card = makeCard({ interval_days: 10 });

    const result = applyIntervalScale(card, 0.5, nowMs);

    expect(result.interval_days).toBe(5);
    expect(result.due_ms).toBe(5 * 86_400_000);
  });
});

describe("checkGraduation", () => {
  it("returns true when repetitions meet the threshold", () => {
    expect(checkGraduation(5, 5)).toBe(true);
  });

  it("returns true when repetitions exceed the threshold", () => {
    expect(checkGraduation(7, 5)).toBe(true);
  });

  it("returns false when repetitions are below the threshold", () => {
    expect(checkGraduation(4, 5)).toBe(false);
  });

  it("returns false when graduationReps is 0 (graduation disabled)", () => {
    expect(checkGraduation(100, 0)).toBe(false);
  });
});
