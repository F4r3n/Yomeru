import type { SrsCard } from "../shared/types.ts";
import { MS_PER_DAY } from "../shared/types.ts";

/** The FSRS subset that lives on every card and that the WASM round-trips. */
export type SrsSchedFields = Pick<
  SrsCard,
  | "due_ms"
  | "stability"
  | "difficulty"
  | "reps"
  | "lapses"
  | "state"
  | "last_review_ms"
>;

/**
 * Merges WASM-updated FSRS fields back into the original card, preserving the
 * composite id / direction / status metadata that the WASM doesn't know about.
 * Forces status to "active" — a reviewed card has graduated from staging.
 */
export function mergeReview(original: SrsCard, reviewed: SrsSchedFields): SrsCard {
  return {
    ...original,
    due_ms: reviewed.due_ms,
    stability: reviewed.stability,
    difficulty: reviewed.difficulty,
    reps: reviewed.reps,
    lapses: reviewed.lapses,
    state: reviewed.state,
    last_review_ms: reviewed.last_review_ms,
    status: "active",
  };
}

/**
 * Scales the freshly-scheduled interval (stability + due_ms) by a constant.
 * Called immediately after `review_card`, so `due_ms` is always `now + interval`
 * — we scale that interval and the underlying stability together so the two
 * stay consistent.
 */
export function applyIntervalScale<T extends SrsSchedFields>(
  card: T,
  scale: number,
  nowMs: number,
): T {
  if (scale === 1.0) return card;
  const intervalDays = (card.due_ms - nowMs) / MS_PER_DAY;
  return {
    ...card,
    stability: card.stability * scale,
    due_ms: nowMs + intervalDays * scale * MS_PER_DAY,
  };
}

/** Returns true when the card should leave the review queue (graduation threshold met). */
export function checkGraduation(reps: number, graduationReps: number): boolean {
  return graduationReps > 0 && reps >= graduationReps;
}
