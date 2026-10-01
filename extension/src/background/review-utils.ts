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
 * Scales the freshly-scheduled interval by a constant. Called immediately after
 * `review_card`, so `due_ms` is always `now + interval`.
 *
 * Stability is left as FSRS computed it. FSRS derives the next stability from
 * the current one, so scaling it here would compound: the scale would apply
 * again on every review (1.5 → ~8× the unscaled interval by the sixth review).
 * Only the due date moves; FSRS sees the longer gap at the next review and
 * adjusts on its own.
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
    due_ms: nowMs + intervalDays * scale * MS_PER_DAY,
  };
}

/**
 * Returns true when the card's freshly-scheduled interval is long enough
 * that it should graduate out of the review queue. No streak tracking
 * needed — a failed review naturally collapses `intervalDays` back below
 * threshold on its own. Mirrors `apply_review` in `app/shared/src/srs.rs`.
 */
export function checkGraduation(intervalDays: number, graduationIntervalDays: number): boolean {
  return graduationIntervalDays > 0 && intervalDays >= graduationIntervalDays;
}
