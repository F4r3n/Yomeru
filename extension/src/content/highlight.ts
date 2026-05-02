const HIGHLIGHT_NAME = "jp-reader-match";

export function initHighlight(): void {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  const s = document.createElement("style");
  s.textContent = `::highlight(${HIGHLIGHT_NAME}) { background-color: rgba(255, 200, 0, 0.45); color: inherit; }`;
  (document.head ?? document.documentElement).appendChild(s);
}

export function setHighlight(
  node: Text,
  charOffset: number,
  matchLen: number,
): void {
  if (typeof CSS === "undefined" || !CSS.highlights || matchLen <= 0) return;
  try {
    const range = new Range();
    range.setStart(node, charOffset);
    range.setEnd(node, charOffset + matchLen);
    CSS.highlights.set(HIGHLIGHT_NAME, new Highlight(range));
  } catch (_) {
    /* unsupported */
  }
}

export function clearHighlight(): void {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  CSS.highlights.delete(HIGHLIGHT_NAME);
}
