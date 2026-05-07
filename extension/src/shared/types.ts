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
  interval_days: number;
  ease_factor: number;
  repetitions: number;
  added_ms: number;
  status: "staging" | "active";
}

export interface SrsSettings {
  maxStagingSize: number;   // 0 = unlimited
  graduationReps: number;   // 0 = never graduate
  intervalScale: number;    // 1.0 = no scaling
  maxSessionCards: number;
}

export const DEFAULT_SETTINGS: SrsSettings = {
  maxStagingSize: 30,
  graduationReps: 0,
  intervalScale: 1.0,
  maxSessionCards: 20,
};

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
  | { type: "GET_SRS_WORDS" }
  | { type: "GET_STAGING" }
  | { type: "PROMOTE_CARD"; payload: { word: string } }
  | { type: "PROMOTE_ALL" }
  | { type: "GET_SETTINGS" }
  | { type: "SAVE_SETTINGS"; payload: SrsSettings }
  | { type: "GET_KANJI"; payload: { word: string } };
