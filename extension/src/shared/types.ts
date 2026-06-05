export interface Gloss {
  text: string;
}

/** `misc` tag discriminants — mirror `Misc` in `jmdict-types/src/entry.rs`
 *  (serialized as u8 via serde_repr). Only the values the extension checks are
 *  listed; add others here as needed, keeping the numbers in sync with Rust. */
export const Misc = {
  UsuallyKana: 51,
} as const;

/** `ke_inf` tag discriminants — mirror `KanjiInf` in `jmdict-types/src/entry.rs`
 *  (serialized as u8 via serde_repr). */
export const KanjiInf = {
  IrregularKanji: 1,
  IrregularOkurigana: 2,
  OutdatedKanji: 3,
  IrregularKana: 4,
  Ateji: 5,
  RareKanji: 6,
  SearchOnlyKanji: 7,
} as const;

/** `ke_pri`/`re_pri` frequency-kind discriminants — mirror `FreqKind` in
 *  `jmdict-types/src/entry.rs` (serialized as u8 via serde_repr). */
export const FreqKind = {
  Gai: 1,
  Ichi: 2,
  News: 3,
  Nf: 4,
  Spec: 5,
} as const;

/** A parsed frequency tag — mirrors `Freq` in `jmdict-types/src/entry.rs`.
 *  e.g. "nf12" → { kind: FreqKind.Nf, value: 12 }, "news1" → { kind: FreqKind.News, value: 1 }. */
export interface Freq {
  kind: number;
  value: number;
}

export interface Sense {
  pos: string[];
  glosses: Gloss[];
  /** `misc` tag discriminants (see {@link Misc}) — most usefully UsuallyKana. */
  misc?: number[];
}

export interface KanjiElement {
  text: string;
  /** `ke_inf` tag discriminants (see {@link KanjiInf}) — e.g. RareKanji. */
  info?: number[];
  /** `ke_pri` frequency tags (see {@link Freq}) — e.g. { kind: News, value: 1 }. */
  priorities?: Freq[];
}

export interface ReadingElement {
  text: string;
  /** `re_pri` frequency tags — same vocabulary as KanjiElement.priorities. */
  priorities?: Freq[];
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
  /** Composite key: `${sequence}::${direction}`. */
  id: string;
  /** JMdict ent_seq of the entry this card reviews. Stable across dict rebuilds. */
  sequence: number;
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

export function cardId(sequence: number, direction: CardDirection): string {
  return `${sequence}::${direction}`;
}

export interface SrsSettings {
  graduationReps: number; // 0 = never graduate
  intervalScale: number; // 1.0 = no scaling
  maxSessionCards: number;
  serverUrl: string;
  serverEmail: string;
  serverToken: string; // session token after OTP verification (not shown to user)
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
