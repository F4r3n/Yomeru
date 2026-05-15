const HIGHLIGHT_NAME = "yomeru-match";

export function initHighlight(): void {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  const s = document.createElement("style");
  s.textContent = `::highlight(${HIGHLIGHT_NAME}) { background-color: rgba(255, 200, 0, 0.45); color: inherit; }`;
  (document.head ?? document.documentElement).appendChild(s);
}

/**
 * Highlight `matchLen` UTF-16 code units of `node` starting at `utf16Offset`.
 * Both values are in UTF-16 code units because DOM Range uses that unit;
 * callers converting from Rust char-offsets must convert first.
 */
export function setHighlight(
  node: Text,
  utf16Offset: number,
  utf16Length: number,
): void {
  if (typeof CSS === "undefined" || !CSS.highlights || utf16Length <= 0) return;
  try {
    const range = new Range();
    range.setStart(node, utf16Offset);
    range.setEnd(node, utf16Offset + utf16Length);
    CSS.highlights.set(HIGHLIGHT_NAME, new Highlight(range));
  } catch (_) {
    /* unsupported */
  }
}

export function clearHighlight(): void {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  CSS.highlights.delete(HIGHLIGHT_NAME);
}
