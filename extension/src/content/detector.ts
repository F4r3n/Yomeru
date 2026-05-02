export interface HitResult {
  node: Text;
  nodeText: string;
  charOffset: number;
}

export function getJapaneseAtPoint(x: number, y: number): HitResult | null {
  const pos = document.caretPositionFromPoint(x, y);
  if (!pos || pos.offsetNode.nodeType !== Node.TEXT_NODE) return null;
  return {
    node: pos.offsetNode as Text,
    nodeText: pos.offsetNode.textContent ?? "",
    charOffset: pos.offset,
  };
}

export function isEditableAt(x: number, y: number): boolean {
  const el = document.elementFromPoint(x, y);
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || (el as HTMLElement).isContentEditable;
}
