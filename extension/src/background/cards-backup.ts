import type { SrsCard } from "../shared/types.ts";
import { getAllCards, getCard, putCard } from "./idb";

export const CARDS_BACKUP_KEY = "_yomeru_cards_backup";

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
  for (const c of backup) await putCard(c);
  return { restored: backup.length, backedUp: 0 };
}

// Skip-existing merge: never clobbers a card the user has been reviewing.
// Returns counts so the UI can report what happened.
export async function importCards(
  cards: unknown,
): Promise<{ added: number; skipped: number; error?: string }> {
  if (!Array.isArray(cards)) {
    return { added: 0, skipped: 0, error: "cards is not an array" };
  }
  let added = 0;
  let skipped = 0;
  for (const card of cards) {
    if (!card || typeof (card as SrsCard).word !== "string") {
      skipped++;
      continue;
    }
    const c = card as SrsCard;
    if (await getCard(c.word)) {
      skipped++;
      continue;
    }
    await putCard(c);
    added++;
  }
  return { added, skipped };
}
