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

/** What a card reviews. Mirrors `CardKind` in `app/core/src/types.rs`. */
export type CardKind = "word" | "kanji" | "grammar";

/** Mirrors `CardDirection` in `app/core/src/types.rs`. Words have
 * `recognition` and `recall`, kanji `meaning`, grammar points `recognition`. */
export type CardDirection = "recognition" | "recall" | "meaning";

/** The directions each kind's cards come in, the order they are made in.
 * Mirrors `CardItem::directions`. */
export const KIND_DIRECTIONS: Record<CardKind, CardDirection[]> = {
  word: ["recognition", "recall"],
  kanji: ["meaning"],
  grammar: ["recognition"],
};

/** Which card, within its kind: a JMdict `ent_seq` (`"1358280"`), a kanji
 * (`"漢"`), a grammar entry's number (`"417"`). Text, whatever the kind,
 * the way the server and the stores hold it. Mirrors `CardItem`. */
export interface CardKey {
  kind: CardKind;
  item: string;
}

/** The key of the word with JMdict `ent_seq` `sequence`. */
export function wordKey(sequence: number): CardKey {
  return { kind: "word", item: String(sequence) };
}

/** Whether a card reviews a JMdict word. */
export function isWordCard(c: { kind: string }): boolean {
  return c.kind === "word";
}

/** Whether `raw` (a stored or downloaded card, of any kind) has the shape
 * every card has: text `id`, `kind`, `item` and `direction`, none empty, the
 * id being `"<kind>:<item>:<direction>"`. One without it can't be stored
 * under its key or sent back (the server refuses it), so it is dropped.
 * Mirrors `is_well_formed_card` in `app/core/src/types.rs`. */
export function isWellFormedCard(raw: unknown): boolean {
  const c = raw as Record<string, unknown> | null;
  if (!c) return false;
  const parts = [c.kind, c.item, c.direction];
  if (!parts.every((p) => typeof p === "string" && p !== "")) return false;
  return c.id === parts.join(":");
}

/** Whether `raw` (a stored or downloaded card) is one this build reads: a
 * known kind with one of that kind's directions. Others, from a newer
 * client, are kept and synced but never shown. Mirrors `WireCard`'s
 * `TryFrom`. */
export function isKnownCard(raw: unknown): raw is SrsCard {
  if (!isWellFormedCard(raw)) return false;
  const c = raw as { kind: string; direction: string };
  const dirs = (KIND_DIRECTIONS as Record<string, CardDirection[] | undefined>)[c.kind];
  return !!dirs && dirs.includes(c.direction as CardDirection);
}

/** Mirrors srs-core's `CardState` enum (serde rename_all = "lowercase"). */
export type CardState = "new" | "learning" | "review" | "relearning";

export interface SrsCard {
  /** `${kind}:${item}:${direction}`, see {@link cardId}. */
  id: string;
  kind: CardKind;
  /** See {@link CardKey}. */
  item: string;
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
  /** "graduated" = FSRS's computed next-review interval reached
   * `SrsSettings.graduationIntervalDays`. Kept (not deleted), excluded from
   * Review, reversible — see `checkGraduation`. */
  status: "staging" | "active" | "graduated";
  /** Bumped when Lookup (in the app) or the hover popup re-clicks "+Priority"
   * on an already-staged word; pushes it up the New Words queue. */
  priority: number;
  /** Every "Again" ever pressed on this card, learning steps included —
   * unlike `lapses`, which only counts forgetting a Review-state card. Ranks
   * the app's trouble-word list. Optional because cards stored before the
   * field existed lack it. Mirrors `SrsCard::again_count` in
   * `app/shared/src/types.rs`. */
  again_count?: number;
  /**
   * Wall-clock ms of the last write to this card on any device, and the sync
   * merge key. Stamped by `putCard`/`putCards` so every mutation advances it.
   *
   * It is deliberately NOT `last_review_ms`: promoting a card to "active" or
   * bumping its priority changes neither, and the app's reset clears
   * `last_review_ms` outright, so keying the merge on the review time let a
   * stale device revert those edits. Optional because cards written before the
   * field existed lack it — read those through `versionMs`.
   */
  updated_ms?: number;
}

/**
 * One answer, held in IndexedDB's review outbox until a sync uploads it. The
 * server keeps the whole log; devices only hold what they haven't sent.
 * Identified by `[card_id, reviewed_ms]`, which is also the outbox key, so a
 * re-sent row is ignored server-side. Mirrors `ReviewLog` in
 * `app/shared/src/types.rs`.
 */
export interface ReviewLog {
  card_id: string;
  /** The button pressed: 1 = Again … 4 = Easy. */
  rating: number;
  /** `"undo"` retracts the answer at `undoes_ms`; the server deletes it. */
  kind: "review" | "drill" | "undo";
  reviewed_ms: number;
  undoes_ms?: number;
}

/** Cap on a staged card's priority. Mirrors `MAX_PRIORITY` in
 * `app/shared/src/types.rs`. */
export const MAX_PRIORITY = 10;

/**
 * Where an entry stands in the SRS, summarised over its two direction cards:
 * active outranks graduated outranks staging, and `priority` is the highest of
 * the two. `"none"` = not in the deck at all.
 */
export interface WordStatus {
  status: "none" | SrsCard["status"];
  priority: number;
}

/**
 * A card's merge version, with the fallback for cards stored before
 * `updated_ms` existed. Both inputs are values every device already agrees on,
 * so the fallback is deterministic rather than an arbitrary tiebreak.
 * Mirrors `SrsCard::version_ms` in `app/shared/src/types.rs`.
 */
export function versionMs(c: SrsCard): number {
  return c.updated_ms && c.updated_ms > 0
    ? c.updated_ms
    : Math.max(c.last_review_ms ?? 0, c.added_ms);
}

/**
 * storage.local key holding a mirror of the whole cards table. Rewritten after
 * every local mutation and every sync, which also makes it the change signal
 * content scripts watch. Lives here so content code needn't import the IDB layer.
 */
export const CARDS_BACKUP_KEY = "_yomeru_cards_backup_v3";

/** Where the mirror lived while cards keyed on `sequence`. Its cards are in
 * the old shape, so it is removed, never restored: the server holds the
 * migrated deck (`docs/phase-4-grammar-cards.md`, "Card keys"). */
export const OLD_CARDS_BACKUP_KEY = "_yomeru_cards_backup";

export function cardId(key: CardKey, direction: CardDirection): string {
  return `${key.kind}:${key.item}:${direction}`;
}

export interface SrsSettings {
  /** Card graduates once FSRS's computed next-review interval reaches this
   * many days. 0 = never graduate. */
  graduationIntervalDays: number;
  intervalScale: number; // 1.0 = no scaling
  maxSessionCards: number;
  /** FSRS desired retention (probability of recall at review time). */
  requestRetention: number;
  /** FSRS weights from the server's optimizer; `null` = the defaults. */
  fsrsParams: number[] | null;
  /**
   * Wall-clock ms of the last *scheduler* edit on this device: the
   * last-write-wins key for settings sync. 0 = never edited here. Mirrors
   * `settings_updated_ms` in `app/shared/src/settings.rs`.
   */
  settingsUpdatedMs: number;
  serverUrl: string;
  serverEmail: string;
  serverToken: string; // session token after OTP verification (not shown to user)
}

export const DEFAULT_SETTINGS: SrsSettings = {
  graduationIntervalDays: 0, // off: a retired card is never tested again
  intervalScale: 1.0,
  maxSessionCards: 20,
  requestRetention: 0.9,
  fsrsParams: null,
  settingsUpdatedMs: 0,
  serverUrl: "",
  serverEmail: "",
  serverToken: "",
};

export interface ExampleEntry {
  japanese: string;
  english: string;
  /** `[start, len]` in code points of the word as written in `japanese`. */
  mark?: [number, number] | null;
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
  /** From the separate mnemonic file; absent when it has none. */
  mnemonic?: string;
}

/** Mirrors `PartInfo` in `kanjidic-types`: one part of the kanji finder's grid. */
export interface PartInfo {
  element: string;
  /** 0 when unknown. */
  strokes: number;
  kanji_count: number;
}

/** Mirrors `PartSearch` in `kanjidic-types`. */
export interface PartSearch {
  kanji: KanjiEntry[];
  total: number;
  open_parts: string[];
}

/** A run of grammar example text — mirrors `Span` in
 *  `grammar-core/src/types.rs` (serde's external tagging, camelCase). */
export type GrammarSpan =
  | { text: { text: string } }
  | { highlight: { text: string } };

export interface GrammarExample {
  ja: GrammarSpan[];
  en: string;
  /** Tatoeba sentence ids `<ja>_<en>`; empty for Yomeru's own sentences. */
  tatoeba: string;
}

/** What the hover popup shows of a grammar point — `PopupEntry` in
 *  `grammar-wasm`. */
export interface GrammarPopupEntry {
  id: string;
  /** The entry's number, the `item` of its review card. */
  seq: number;
  pattern: string;
  jlpt: string;
  meaning: string;
  /** How it attaches, one line per construction. */
  formation: string[];
  examples: GrammarExample[];
}

export interface GrammarPopupMatch {
  entries: GrammarPopupEntry[];
  /** Chars of the hovered text the pattern covers. */
  matchLen: number;
}
