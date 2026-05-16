import type { SrsCard } from "../shared/types.ts";

/** The SM-2 subset that lives on every card and that the WASM round-trips. */
export type SrsSchedFields = Pick<
  SrsCard,
  "due_ms" | "interval_days" | "ease_factor" | "repetitions"
>;

/**
 * Merges WASM-updated SM-2 fields back into the original card, preserving the
 * composite id / direction / status metadata that the WASM doesn't know about.
 * Forces status to "active" — a reviewed card has graduated from staging.
 */
export function mergeReview(original: SrsCard, reviewed: SrsSchedFields): SrsCard {
  return {
    ...original,
    due_ms: reviewed.due_ms,
    interval_days: reviewed.interval_days,
    ease_factor: reviewed.ease_factor,
    repetitions: reviewed.repetitions,
    status: "active",
  };
}

/** Scales interval_days and recomputes due_ms. Returns the input unchanged when scale === 1.0. */
export function applyIntervalScale<T extends SrsSchedFields>(
  card: T,
  scale: number,
  nowMs: number,
): T {
  if (scale === 1.0) return card;
  const scaledDays = card.interval_days * scale;
  return { ...card, interval_days: scaledDays, due_ms: nowMs + scaledDays * 86_400_000 };
}

/** Returns true when the card should leave the review queue (graduation threshold met). */
export function checkGraduation(repetitions: number, graduationReps: number): boolean {
  return graduationReps > 0 && repetitions >= graduationReps;
}
