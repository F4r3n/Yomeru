import type { SrsCard } from "../shared/types.ts";
import { CARDS_BACKUP_KEY, OLD_CARDS_BACKUP_KEY, cardId } from "../shared/types.ts";
import { getAllRawCards, getCardsByIds, putCard } from "./idb";
import { debug } from "../shared/debug.ts";

export { CARDS_BACKUP_KEY };

/**
 * An input card as stored, or `null` when it isn't one: cards key on `kind` +
 * `item` + `direction`, all three text. A card in the shape from before (a
 * numeric `sequence`, ids `"<sequence>::<direction>"`) is no card here: the
 * app's import converts those exports (`cards_io::parse_v2_cards`). A kind
 * or direction this build doesn't know is kept, like sync keeps it.
 */
export function normalizeImportedCard(card: unknown): SrsCard | null {
  const c = card as Partial<SrsCard> | null;
  if (!c || typeof c.kind !== "string" || typeof c.item !== "string") return null;
  if (typeof c.direction !== "string") return null;
  const id = cardId({ kind: c.kind, item: c.item }, c.direction);
  if (c.id !== undefined && c.id !== id) return null;
  return { ...(c as SrsCard), id };
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
  const cards = await getAllRawCards();
  if (cards.length === 0) return;
  await browser.storage.local.set({ [CARDS_BACKUP_KEY]: cards });
}

// On startup: if IDB has cards → refresh the backup so first-time users have
// one before any mutation; if IDB is empty but a backup exists → restore.
// IDB always wins when both have data — the backup is a strict mirror, never
// authoritative against a live IDB.
//
// A mirror from before the card key change is removed, not restored: its
// cards are in the old shape, and the server holds the converted deck, which
// the first sync after the upgrade pulls back.
export async function syncCardsBackup(): Promise<{ restored: number; backedUp: number }> {
  await browser.storage.local.remove(OLD_CARDS_BACKUP_KEY);
  const idbCards = await getAllRawCards();
  debug(`syncCardsBackup: ${idbCards.length} card(s) in IDB`);
  if (idbCards.length > 0) {
    await browser.storage.local.set({ [CARDS_BACKUP_KEY]: idbCards });
    return { restored: 0, backedUp: idbCards.length };
  }
  const stored = await browser.storage.local.get(CARDS_BACKUP_KEY);
  const backup = (stored as { [k: string]: SrsCard[] | undefined })[CARDS_BACKUP_KEY];
  if (!backup || backup.length === 0) {
    debug("syncCardsBackup: no storage.local backup to restore");
    return { restored: 0, backedUp: 0 };
  }
  console.warn(
    `[yomeru] IDB empty, restoring ${backup.length} card(s) from storage.local backup`,
  );
  let restored = 0;
  for (const c of backup) {
    const card = normalizeImportedCard(c);
    if (!card) continue;
    await putCard(card);
    restored++;
  }
  debug(`syncCardsBackup: restored ${restored} card(s) from backup`);
  return { restored, backedUp: 0 };
}

// Skip-existing merge: never clobbers a card the user has been reviewing.
// Returns counts so the UI can report what happened, one per input row.
export async function importCards(
  cards: unknown,
): Promise<{ added: number; skipped: number; error?: string }> {
  if (!Array.isArray(cards)) {
    return { added: 0, skipped: 0, error: "cards is not an array" };
  }
  let added = 0;
  let skipped = 0;
  for (const raw of cards) {
    const card = normalizeImportedCard(raw);
    if (!card || (await getCardsByIds([card.id])).length > 0) {
      skipped++;
      continue;
    }
    await putCard(card);
    added++;
  }
  return { added, skipped };
}
