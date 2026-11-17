import { beforeEach, describe, expect, it, vi } from "vitest";

type HighlighterModule = typeof import("./srs-highlighter.ts");

describe("applyCardsChange", () => {
  let hl: HighlighterModule;

  beforeEach(async () => {
    vi.resetModules();
    hl = await import("./srs-highlighter.ts");
  });

  it("drops a deleted entry so it stops being underlined", () => {
    hl.applyCardsChange([{ kind: "word", item: "1" }, { kind: "word", item: "2" }]);

    expect(hl.applyCardsChange([{ kind: "word", item: "1" }])).toBe(true);
    expect(hl.hasSrsSequence(2)).toBe(false);
    expect(hl.hasSrsSequence(1)).toBe(true);
  });

  it("picks up an entry added elsewhere", () => {
    expect(hl.applyCardsChange([{ kind: "word", item: "7" }])).toBe(true);
    expect(hl.hasSrsSequence(7)).toBe(true);
  });

  it("reports no change when only card contents changed", () => {
    // A review rewrites the backup without touching which entries exist; the
    // full-page rescan must be skipped then.
    hl.applyCardsChange([{ kind: "word", item: "1", reps: 1 }, { kind: "word", item: "1", reps: 0 }]);

    expect(hl.applyCardsChange([{ kind: "word", item: "1", reps: 2 }])).toBe(false);
  });

  it("leaves kanji and grammar cards out: they are no entry", () => {
    expect(
      hl.applyCardsChange([
        { kind: "kanji", item: "猫" },
        { kind: "grammar", item: "7" },
      ]),
    ).toBe(false);
    expect(hl.hasSrsSequence(7)).toBe(false);
  });

  it("treats a cleared backup as no saved words", () => {
    hl.applyCardsChange([{ kind: "word", item: "1" }]);

    expect(hl.applyCardsChange(undefined)).toBe(true);
    expect(hl.hasSrsSequence(1)).toBe(false);
  });
});
