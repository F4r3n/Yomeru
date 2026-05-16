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

export type CardDirection = "recognition" | "recall";

export interface SrsCard {
  /** Composite key: `${word}::${direction}`. */
  id: string;
  word: string;
  direction: CardDirection;
  due_ms: number;
  interval_days: number;
  ease_factor: number;
  repetitions: number;
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
}

export const DEFAULT_SETTINGS: SrsSettings = {
  graduationReps: 0,
  intervalScale: 1.0,
  maxSessionCards: 20,
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

