import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import { getAllCards, getCard, putCard } from "./idb";

export const CARDS_BACKUP_KEY = "_yomeru_cards_backup";

/**
 * Expands one input card into one or two v3 siblings.
 *
 * - v3 input (has `direction`): returned as-is, with `id` filled in if missing.
 * - Legacy input (no `direction`): becomes a recognition sibling preserving the
 *   original SM-2 state plus a fresh recall sibling (active, due now). Matches
 *   the v2→v3 IDB migration so import and migrate produce the same end state.
 */
export function normalizeImportedCard(card: SrsCard, nowMs: number): SrsCard[] {
  if (card.direction) {
    return [{ ...card, id: card.id ?? cardId(card.word, card.direction) }];
  }
  const recognition: SrsCard = {
    ...card,
    id: cardId(card.word, "recognition"),
    direction: "recognition",
  };
  const recall: SrsCard = {
    ...card,
    id: cardId(card.word, "recall"),
    direction: "recall",
    due_ms: nowMs,
    interval_days: 0,
    ease_factor: 2.5,
    repetitions: 0,
    status: "active",
  };
  return [recognition, recall];
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
