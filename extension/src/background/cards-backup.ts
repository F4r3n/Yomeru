import type { CardDirection, SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import { freshRecallCard, getAllCards, getCard, putCard, sm2ToFsrsFields } from "./idb";

export const CARDS_BACKUP_KEY = "_yomeru_cards_backup";

function num(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

function buildFromLegacy(
  raw: Record<string, unknown>,
  word: string,
  direction: CardDirection,
  nowMs: number,
): SrsCard {
  return {
    id: cardId(word, direction),
    word,
    direction,
    due_ms: num(raw.due_ms, nowMs),
    ...sm2ToFsrsFields(raw),
    added_ms: num(raw.added_ms, nowMs),
    status: (raw.status as SrsCard["status"]) ?? "active",
  };
}

/**
 * Normalize an input card to one or two v4 (FSRS-shaped) siblings.
 *
 * - v4 input (already FSRS-shaped): returned as-is, with `id` filled in if missing.
 * - v3 input (has `direction` but SM-2 fields): direction preserved, SM-2 → FSRS mapped.
 * - Legacy input (no `direction`): becomes a recognition sibling preserving the
 *   original SM-2 state plus a fresh recall sibling. Matches the v2→v4 IDB
 *   migration so import and migrate produce the same end state.
 */
export function normalizeImportedCard(card: SrsCard, nowMs: number): SrsCard[] {
  const raw = card as unknown as Record<string, unknown>;
  const isLegacyShape =
    typeof raw.stability !== "number" ||
    typeof raw.difficulty !== "number" ||
    typeof raw.state !== "string";

  if (card.direction && !isLegacyShape) {
    return [{ ...card, id: card.id ?? cardId(card.word, card.direction) }];
  }
  if (card.direction) {
    return [buildFromLegacy(raw, card.word, card.direction, nowMs)];
  }
  const recognition = buildFromLegacy(raw, card.word, "recognition", nowMs);
  return [recognition, freshRecallCard(card.word, nowMs, recognition.added_ms)];
}

// Mirrors the IDB cards table to storage.local. Called on every mutation so a
// Firefox-side IDB wipe (uninstall, reinstall via a different path, temp
// add-on teardown) doesn't vaporize the user's SRS deck — storage.local
// survives more reinstall scenarios than IDB.
export async function writeCardsBackup(): Promise<void> {
  const cards = await getAllCards();
  await browser.storage.local.set({ [CARDS_BACKUP_KEY]: cards });
}

// On startup: if IDB has cards → refresh the backup so first-time users have
// one before any mutation; if IDB is empty but a backup exists → restore.
// IDB always wins when both have data — the backup is a strict mirror, never
// authoritative against a live IDB.
export async function syncCardsBackup(): Promise<{ restored: number; backedUp: number }> {
  const idbCards = await getAllCards();
  if (idbCards.length > 0) {
    await browser.storage.local.set({ [CARDS_BACKUP_KEY]: idbCards });
    return { restored: 0, backedUp: idbCards.length };
  }
  const stored = await browser.storage.local.get(CARDS_BACKUP_KEY);
  const backup = (stored as { [k: string]: SrsCard[] | undefined })[CARDS_BACKUP_KEY];
  if (!backup || backup.length === 0) return { restored: 0, backedUp: 0 };
  console.warn(
    `[yomeru] IDB empty, restoring ${backup.length} card(s) from storage.local backup`,
  );
  const now = Date.now();
  let restored = 0;
  for (const c of backup) {
    for (const sibling of normalizeImportedCard(c, now)) {
      await putCard(sibling);
      restored++;
    }
  }
  return { restored, backedUp: 0 };
}

// Skip-existing merge: never clobbers a card the user has been reviewing.
// Returns counts so the UI can report what happened. Counts are per *input*
// row, not per sibling — a legacy row that expands to two siblings still
// counts as one added/skipped, matching the user's mental model of the JSON.
export async function importCards(
  cards: unknown,
): Promise<{ added: number; skipped: number; error?: string }> {
  if (!Array.isArray(cards)) {
    return { added: 0, skipped: 0, error: "cards is not an array" };
  }
  const now = Date.now();
  let added = 0;
  let skipped = 0;
  for (const card of cards) {
    if (!card || typeof (card as SrsCard).word !== "string") {
      skipped++;
      continue;
    }
    const expanded = normalizeImportedCard(card as SrsCard, now);
    let importedAny = false;
    for (const sibling of expanded) {
      if (await getCard(sibling.word, sibling.direction)) continue;
      await putCard(sibling);
      importedAny = true;
    }
    if (importedAny) added++;
    else skipped++;
  }
  return { added, skipped };
}
