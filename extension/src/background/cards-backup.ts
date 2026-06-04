import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import { freshRecallCard, getAllCards, getCard, putCard } from "./idb";

export const CARDS_BACKUP_KEY = "_yomeru_cards_backup";

function num(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

/**
 * Normalize an input card to one or two sequence-keyed (FSRS-shaped) siblings.
 *
 * Cards key on JMdict `sequence`; a row without a numeric `sequence` is from
 * the pre-`sequence` (word-keyed) era and cannot be mapped, so it's dropped —
 * this is the clean break, consistent with users re-exporting in the new shape.
 *
 * - With a `direction`: returned as-is, with `id` filled in if missing.
 * - Without a `direction`: becomes a recognition sibling preserving its
 *   scheduling state plus a fresh recall sibling.
 */
export function normalizeImportedCard(card: SrsCard, nowMs: number): SrsCard[] {
  const raw = card as unknown as Record<string, unknown>;
  if (typeof raw.sequence !== "number") return [];

  if (card.direction) {
    return [{ ...card, id: card.id ?? cardId(card.sequence, card.direction) }];
  }
  const recognition: SrsCard = {
    ...card,
    id: cardId(card.sequence, "recognition"),
    direction: "recognition",
    added_ms: num(raw.added_ms, nowMs),
  };
  return [recognition, freshRecallCard(card.sequence, nowMs, recognition.added_ms)];
}

// Mirrors the IDB cards table to storage.local. Called on every mutation so a
// Firefox-side IDB wipe (uninstall, reinstall via a different path, temp
// add-on teardown) doesn't vaporize the user's SRS deck — storage.local
// survives more reinstall scenarios than IDB.
//
// Safety: if IDB is empty we skip the write. An empty IDB is almost always a
// transient state (failed migration, fresh load before syncCardsBackup has
// restored) — overwriting a valid backup with [] in that window is how we lost
// a user's deck. The trade-off is that genuine "delete all cards" never trims
// the backup, which is the right side to err on.
export async function writeCardsBackup(): Promise<void> {
  const cards = await getAllCards();
  if (cards.length === 0) return;
  await browser.storage.local.set({ [CARDS_BACKUP_KEY]: cards });
}

// On startup: if IDB has cards → refresh the backup so first-time users have
// one before any mutation; if IDB is empty but a backup exists → restore.
// IDB always wins when both have data — the backup is a strict mirror, never
// authoritative against a live IDB.
export async function syncCardsBackup(): Promise<{ restored: number; backedUp: number }> {
  const idbCards = await getAllCards();
  console.log(`[yomeru] syncCardsBackup: ${idbCards.length} card(s) in IDB`);
  if (idbCards.length > 0) {
    await browser.storage.local.set({ [CARDS_BACKUP_KEY]: idbCards });
    return { restored: 0, backedUp: idbCards.length };
  }
  const stored = await browser.storage.local.get(CARDS_BACKUP_KEY);
  const backup = (stored as { [k: string]: SrsCard[] | undefined })[CARDS_BACKUP_KEY];
  if (!backup || backup.length === 0) {
    console.log("[yomeru] syncCardsBackup: no storage.local backup to restore");
    return { restored: 0, backedUp: 0 };
  }
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
  console.log(`[yomeru] syncCardsBackup: restored ${restored} sibling(s) from backup`);
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
    if (!card || typeof (card as SrsCard).sequence !== "number") {
      skipped++;
      continue;
    }
    const expanded = normalizeImportedCard(card as SrsCard, now);
    let importedAny = false;
    for (const sibling of expanded) {
      if (await getCard(sibling.sequence, sibling.direction)) continue;
      await putCard(sibling);
      importedAny = true;
    }
    if (importedAny) added++;
    else skipped++;
  }
  return { added, skipped };
}
