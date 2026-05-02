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
  const checkOffset = Math.min(pos.offset, textLen - 1);
  const range = document.createRange();
  range.setStart(node, checkOffset);
  range.setEnd(node, checkOffset + 1);
  const rect = range.getBoundingClientRect();

  const MARGIN = 2;
  if (
    x < rect.left - MARGIN ||
    x > rect.right + MARGIN ||
    y < rect.top - MARGIN ||
    y > rect.bottom + MARGIN
  ) {
    return null;
  }

  return { node, nodeText: node.textContent ?? "", charOffset: pos.offset };
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
