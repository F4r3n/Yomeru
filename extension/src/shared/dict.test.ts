import { describe, expect, it } from "vitest";
import { preferredHeadword } from "./dict.ts";
import { Misc } from "./types.ts";
import type { WordEntry } from "./types.ts";

const UK = [Misc.UsuallyKana];

function entry(kanji: string, reading: string, senseMisc: number[][]): WordEntry {
  return {
    sequence: 1,
    kanji_forms: [{ text: kanji }],
    reading_forms: [{ text: reading }],
    senses: senseMisc.map((misc) => ({ pos: [], glosses: [], misc })),
  };
}

describe("preferredHeadword", () => {
  it("keeps the kanji when only a later sense is usually kana", () => {
    // 猫: "cat" first, "shamisen" (uk) much later.
    expect(preferredHeadword(entry("猫", "ねこ", [[], [], UK]))).toBe("猫");
  });

  it("uses the reading when the main sense is usually kana", () => {
    expect(preferredHeadword(entry("宜しく", "よろしく", [UK, UK]))).toBe("よろしく");
    expect(preferredHeadword(entry("有難う", "ありがとう", [UK]))).toBe("ありがとう");
  });

  it("keeps the kanji when no sense is usually kana", () => {
    expect(preferredHeadword(entry("学校", "がっこう", [[]]))).toBe("学校");
  });
});
