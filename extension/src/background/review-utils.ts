import type { SrsCard } from "../shared/types.ts";

/** Merges WASM-updated scheduling fields back onto the original card, preserving JS-only fields like senses. */
export function mergeReview(original: SrsCard, reviewed: SrsCard): SrsCard {
  return { ...reviewed, senses: original.senses, status: original.status };
}

/** Scales interval_days and recomputes due_ms. Returns the original card unchanged when scale === 1.0. */
export function applyIntervalScale(
  card: SrsCard,
  scale: number,
  nowMs: number,
): SrsCard {
  if (scale === 1.0) return card;
  const scaledDays = card.interval_days * scale;
  return { ...card, interval_days: scaledDays, due_ms: nowMs + scaledDays * 86_400_000 };
}

/** Returns true when the card should leave the review queue (graduation threshold met). */
export function checkGraduation(repetitions: number, graduationReps: number): boolean {
  return graduationReps > 0 && repetitions >= graduationReps;
}
