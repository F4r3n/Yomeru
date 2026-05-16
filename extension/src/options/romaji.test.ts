import { describe, it, expect } from "vitest";
import { isRomaji, romajiToHiragana } from "./romaji.ts";

describe("isRomaji", () => {
  it("accepts plain ASCII letters", () => {
    expect(isRomaji("taberu")).toBe(true);
    expect(isRomaji("Taberu")).toBe(true);
    expect(isRomaji("a")).toBe(true);
  });

  it("accepts hyphen for long-vowel-style input", () => {
    expect(isRomaji("oo-kii")).toBe(true);
  });

  it("rejects empty input", () => {
    expect(isRomaji("")).toBe(false);
  });

  it("rejects Japanese / mixed / numeric input", () => {
    expect(isRomaji("食べる")).toBe(false);
    expect(isRomaji("たべる")).toBe(false);
    expect(isRomaji("ka1")).toBe(false);
    expect(isRomaji("ka る")).toBe(false);
  });
});

describe("romajiToHiragana", () => {
  it("converts plain vowels", () => {
    expect(romajiToHiragana("aiueo")).toBe("あいうえお");
  });

  it("converts basic syllables", () => {
    expect(romajiToHiragana("taberu")).toBe("たべる");
    expect(romajiToHiragana("nomu")).toBe("のむ");
    expect(romajiToHiragana("hana")).toBe("はな");
  });

  it("handles the special syllables shi/chi/tsu/fu/ji", () => {
    expect(romajiToHiragana("shi")).toBe("し");
    expect(romajiToHiragana("chi")).toBe("ち");
    expect(romajiToHiragana("tsu")).toBe("つ");
    expect(romajiToHiragana("fu")).toBe("ふ");
    expect(romajiToHiragana("ji")).toBe("じ");
  });

  it("handles yōon (kya, sha, cho, ryu, ...)", () => {
    expect(romajiToHiragana("kyou")).toBe("きょう");
    expect(romajiToHiragana("shashin")).toBe("しゃしん");
    expect(romajiToHiragana("ryokou")).toBe("りょこう");
    expect(romajiToHiragana("jisho")).toBe("じしょ");
  });

  it("doubles consonants via small つ", () => {
    expect(romajiToHiragana("matte")).toBe("まって");
    expect(romajiToHiragana("ikko")).toBe("いっこ");
    expect(romajiToHiragana("kitte")).toBe("きって");
  });

  it("handles bare n and n-before-consonant", () => {
    expect(romajiToHiragana("san")).toBe("さん");
    expect(romajiToHiragana("hon")).toBe("ほん");
    expect(romajiToHiragana("ganbaru")).toBe("がんばる");
  });

  it("handles konnichiwa correctly (n then ni)", () => {
    expect(romajiToHiragana("konnichiwa")).toBe("こんにちは"
      // "wa" → わ in our table; the greeting uses は (particle) but mapping
      // is purely phonetic, so we expect わ here.
      .replace("は", "わ"));
  });

  it("is case-insensitive", () => {
    expect(romajiToHiragana("Taberu")).toBe("たべる");
    expect(romajiToHiragana("TABERU")).toBe("たべる");
  });

  it("passes through unrecognized characters", () => {
    expect(romajiToHiragana("ka-")).toBe("か-");
  });

  it("returns empty for empty input", () => {
    expect(romajiToHiragana("")).toBe("");
  });
});
