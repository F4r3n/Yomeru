// Standard Hepburn romaji → hiragana. Greedy longest-match (3 → 2 → 1 chars).
// Handles double consonants via small つ and `n` standalone.
const MAP: Record<string, string> = {
  kya: "きゃ", kyu: "きゅ", kyo: "きょ",
  sha: "しゃ", shu: "しゅ", sho: "しょ", shi: "し",
  cha: "ちゃ", chu: "ちゅ", cho: "ちょ", chi: "ち", tsu: "つ",
  nya: "にゃ", nyu: "にゅ", nyo: "にょ",
  hya: "ひゃ", hyu: "ひゅ", hyo: "ひょ",
  mya: "みゃ", myu: "みゅ", myo: "みょ",
  rya: "りゃ", ryu: "りゅ", ryo: "りょ",
  gya: "ぎゃ", gyu: "ぎゅ", gyo: "ぎょ",
  ja: "じゃ", ju: "じゅ", jo: "じょ", ji: "じ",
  jya: "じゃ", jyu: "じゅ", jyo: "じょ",
  bya: "びゃ", byu: "びゅ", byo: "びょ",
  pya: "ぴゃ", pyu: "ぴゅ", pyo: "ぴょ",
  ka: "か", ki: "き", ku: "く", ke: "け", ko: "こ",
  ga: "が", gi: "ぎ", gu: "ぐ", ge: "げ", go: "ご",
  sa: "さ", su: "す", se: "せ", so: "そ",
  za: "ざ", zu: "ず", ze: "ぜ", zo: "ぞ",
  ta: "た", te: "て", to: "と",
  da: "だ", de: "で", do: "ど",
  na: "な", ni: "に", nu: "ぬ", ne: "ね", no: "の",
  ha: "は", hi: "ひ", fu: "ふ", he: "へ", ho: "ほ",
  ba: "ば", bi: "び", bu: "ぶ", be: "べ", bo: "ぼ",
  pa: "ぱ", pi: "ぴ", pu: "ぷ", pe: "ぺ", po: "ぽ",
  ma: "ま", mi: "み", mu: "む", me: "め", mo: "も",
  ya: "や", yu: "ゆ", yo: "よ",
  ra: "ら", ri: "り", ru: "る", re: "れ", ro: "ろ",
  wa: "わ", wo: "を",
  a: "あ", i: "い", u: "う", e: "え", o: "お",
  n: "ん",
};

const ROMAJI_RE = /^[a-zA-Z\-]+$/;

export function isRomaji(s: string): boolean {
  return s.length > 0 && ROMAJI_RE.test(s);
}

export function romajiToHiragana(input: string): string {
  const s = input.toLowerCase();
  let out = "";
  let i = 0;
  while (i < s.length) {
    // Double consonant (except 'n') → small っ
    const c = s[i];
    if (c !== "n" && c >= "a" && c <= "z" && c === s[i + 1] && !"aeiou".includes(c)) {
      out += "っ";
      i++;
      continue;
    }
    // 'n' followed by a consonant (or end-of-string) → ん
    if (c === "n" && (i + 1 >= s.length || !"aeiouy".includes(s[i + 1]))) {
      out += "ん";
      i++;
      continue;
    }
    // Greedy longest-match: 3 → 2 → 1
    let matched = false;
    for (const len of [3, 2, 1]) {
      const chunk = s.slice(i, i + len);
      if (MAP[chunk]) {
        out += MAP[chunk];
        i += len;
        matched = true;
        break;
      }
    }
    if (!matched) {
      out += s[i];
      i++;
    }
  }
  return out;
}
