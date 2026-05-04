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

export interface SrsCard {
  word: string;
  reading: string;
  meaning_en: string;
  senses?: Sense[];
  due_ms: number;
  interval: number;
  ease: number;
  reps: number;
  added_ms: number;
}

// ── Message bus ──────────────────────────────────────────────────────────────

export interface AddWordPayload {
  word: string;
  reading: string;
  meaning_en: string;
  senses?: Sense[];
}
export interface ReviewCardPayload {
  word: string;
  rating: number;
}
export interface DeleteCardPayload {
  word: string;
}
export interface LogLookupPayload {
  word: string;
  reading: string;
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

export type ExtMessage =
  | { type: "ADD_WORD"; payload: AddWordPayload }
  | { type: "REVIEW_CARD"; payload: ReviewCardPayload }
  | { type: "GET_DUE" }
  | { type: "GET_ALL_CARDS" }
  | { type: "DELETE_CARD"; payload: DeleteCardPayload }
  | { type: "LOG_LOOKUP"; payload: LogLookupPayload }
  | { type: "GET_SRS_WORDS" };
