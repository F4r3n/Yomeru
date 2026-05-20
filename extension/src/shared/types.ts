export interface Gloss {
  text: string;
}

export interface Sense {
  pos: string[];
  glosses: Gloss[];
}

export interface KanjiElement {
  text: string;
}

export interface ReadingElement {
  text: string;
}

export interface WordEntry {
  sequence: number;
  kanji_forms: KanjiElement[];
  reading_forms: ReadingElement[];
  senses: Sense[];
}

export const MS_PER_DAY = 86_400_000;

export type CardDirection = "recognition" | "recall";

/** Mirrors srs-core's `CardState` enum (serde rename_all = "lowercase"). */
export type CardState = "new" | "learning" | "review" | "relearning";

export interface SrsCard {
  /** Composite key: `${word}::${direction}`. */
  id: string;
  word: string;
  direction: CardDirection;
  due_ms: number;
  /** FSRS memory stability (days) — interval is derived from this + retention target. */
  stability: number;
  /** FSRS difficulty (1..10). */
  difficulty: number;
  /** Total review count. */
  reps: number;
  /** Number of times the card has been forgotten (Again on a Review-state card). */
  lapses: number;
  state: CardState;
  last_review_ms: number | null;
  added_ms: number;
  status: "staging" | "active";
}

export function cardId(word: string, direction: CardDirection): string {
  return `${word}::${direction}`;
}

export interface SrsSettings {
  graduationReps: number;   // 0 = never graduate
  intervalScale: number;    // 1.0 = no scaling
  maxSessionCards: number;
  serverUrl: string;
  serverEmail: string;
  serverToken: string;      // session token after OTP verification (not shown to user)
}

export const DEFAULT_SETTINGS: SrsSettings = {
  graduationReps: 0,
  intervalScale: 1.0,
  maxSessionCards: 20,
  serverUrl: "",
  serverEmail: "",
  serverToken: "",
};

export interface ExampleEntry {
  japanese: string;
  english: string;
}

export interface KanjiEntry {
  literal: string;
  stroke_count: number;
  grade: number | null;
  freq: number | null;
  jlpt: number | null;
  on_readings: string[];
  kun_readings: string[];
  meanings: string[];
}

