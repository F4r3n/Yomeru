export interface Gloss {
  text: string;
  lang: string;
  gloss_type: string | null;
}

export interface Sense {
  pos: string[];
  glosses: Gloss[];
  xrefs: string[];
  antonyms: string[];
  fields: string[];
  misc: string[];
  info: string[];
  dialects: string[];
}

export interface KanjiElement {
  text: string;
  info: string[];
  priorities: string[];
}

export interface ReadingElement {
  text: string;
  no_kanji: boolean;
  restricted_to: string[];
  info: string[];
  priorities: string[];
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
  due_ms: number;
  interval: number;
  ease: number;
  reps: number;
  added_ms: number;
}

// ── Message bus ──────────────────────────────────────────────────────────────

export interface AddWordPayload { word: string; reading: string; meaning_en: string; }
export interface ReviewCardPayload { word: string; rating: number; }
export interface DeleteCardPayload { word: string; }
export interface LogLookupPayload { word: string; reading: string; }

export type ExtMessage =
  | { type: "ADD_WORD";    payload: AddWordPayload }
  | { type: "REVIEW_CARD"; payload: ReviewCardPayload }
  | { type: "GET_DUE" }
  | { type: "GET_ALL_CARDS" }
  | { type: "DELETE_CARD"; payload: DeleteCardPayload }
  | { type: "LOG_LOOKUP";  payload: LogLookupPayload };
