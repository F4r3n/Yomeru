/**
 * Lays the optional files that sit beside kanjidic.bin over `dict`: the
 * mnemonics and the plain part names. Like the examples data, a missing or
 * broken file just means kanji come without them.
 */
export async function loadKanjiOverlays(dict: {
  withMnemonics(tsv: string): void;
  withPartNames(tsv: string): void;
}): Promise<void> {
  await Promise.all([
    loadOverlay("kanji-mnemonics.tsv", (t) => dict.withMnemonics(t)),
    loadOverlay("part-names.tsv", (t) => dict.withPartNames(t)),
  ]);
}

async function loadOverlay(file: string, apply: (tsv: string) => void) {
  try {
    const resp = await fetch(browser.runtime.getURL(`data/${file}`));
    if (resp.ok) apply(await resp.text());
  } catch (e) {
    console.warn(`[yomeru] ${file} unavailable:`, e);
  }
}
