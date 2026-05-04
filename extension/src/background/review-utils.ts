import type { SrsCard } from "../shared/types.ts";

/** Merges WASM-updated scheduling fields back onto the original card, preserving JS-only fields like senses. */
export function mergeReview(original: SrsCard, reviewed: SrsCard): SrsCard {
  return { ...reviewed, senses: original.senses };
}
