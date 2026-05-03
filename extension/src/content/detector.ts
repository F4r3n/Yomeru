export interface HitResult {
  node: Text;
  nodeText: string;
  charOffset: number;
}

export function getJapaneseAtPoint(x: number, y: number): HitResult | null {
  const pos = document.caretPositionFromPoint(x, y);
  if (!pos || pos.offsetNode.nodeType !== Node.TEXT_NODE) return null;

  const node = pos.offsetNode as Text;
  const textLen = node.textContent?.length ?? 0;
  if (textLen === 0) return null;

  // caretPositionFromPoint snaps to the nearest caret position even when the
  // mouse is far from the text. Verify the mouse is actually over the character
  // by checking the bounding rect of the character at the caret position.
  //
  // The caret lands on the LEADING edge of char[n], so when the mouse is over
  // the TRAILING edge of char[n-1] the primary check (char[n]) fails. We fall
  // back to char[n-1] to handle that case.
  const MARGIN = 2;
  const candidates = [...new Set([
    Math.min(pos.offset, textLen - 1),
    pos.offset - 1,
  ])].filter((o) => o >= 0);

  for (const offset of candidates) {
    const range = document.createRange();
    range.setStart(node, offset);
    range.setEnd(node, offset + 1);
    const rect = range.getBoundingClientRect();
    if (
      x >= rect.left - MARGIN &&
      x <= rect.right + MARGIN &&
      y >= rect.top - MARGIN &&
      y <= rect.bottom + MARGIN
    ) {
      return { node, nodeText: node.textContent ?? "", charOffset: offset };
    }
  }

  return null;
}

export function isEditableAt(x: number, y: number): boolean {
  const el = document.elementFromPoint(x, y);
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return (
    tag === "input" ||
    tag === "textarea" ||
    (el as HTMLElement).isContentEditable
  );
}
