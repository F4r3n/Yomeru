import { beforeEach, describe, expect, it, vi } from "vitest";
import { wordKey } from "../shared/types.ts";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";
import { OLD_CARDS_BACKUP_KEY } from "../shared/types.ts";
import { makeCard } from "./test-helpers.ts";
import { readFileSync } from "node:fs";

/** The cards of a schema-2 export, keyed on `sequence`: the shape the
 * storage.local snapshot held before the card key change. */
const EXPORT_V2: unknown[] = JSON.parse(
  readFileSync(new URL("../../../testdata/backup/export-v2.json", import.meta.url), "utf8"),
).cards;

const byId = (a: SrsCard, b: SrsCard) => a.id.localeCompare(b.id);

type IdbModule = typeof import("./idb.ts");
type CardsBackupModule = typeof import("./cards-backup.ts");

const CAT = 1_001;
const DOG = 1_002;

describe("cards-backup", () => {
  let idb: IdbModule;
  let backup: CardsBackupModule;
  let storage: Map<string, unknown>;

  beforeEach(async () => {
    vi.resetModules();
    globalThis.indexedDB = new IDBFactory();
    (globalThis as unknown as Record<string, unknown>).IDBKeyRange = FakeIDBKeyRange;
    storage = new Map();
    vi.stubGlobal("browser", {
      storage: {
        local: {
          get: async (key: string) => {
            const v = storage.get(key);
            return v !== undefined ? { [key]: v } : {};
          },
          set: async (obj: Record<string, unknown>) => {
            for (const [k, v] of Object.entries(obj)) storage.set(k, v);
          },
          remove: async (key: string) => {
            storage.delete(key);
          },
        },
      },
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    idb = await import("./idb.ts");
    backup = await import("./cards-backup.ts");
  });

  describe("writeCardsBackup", () => {
    it("does not overwrite a backup when IDB is empty", async () => {
      const existing = [makeCard({ sequence: 42 })];
      storage.set(backup.CARDS_BACKUP_KEY, existing);

      await backup.writeCardsBackup();

      expect(storage.get(backup.CARDS_BACKUP_KEY)).toEqual(existing);
    });

    it("does not create a backup when IDB is empty and none exists", async () => {
      await backup.writeCardsBackup();
      expect(storage.has(backup.CARDS_BACKUP_KEY)).toBe(false);
    });

    it("snapshots every IDB card into storage.local", async () => {
      await idb.putCard(makeCard({ sequence: CAT, reps: 3 }));
      await idb.putCard(makeCard({ sequence: DOG, status: "staging" }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(2);
      expect(stored.map((c) => Number(c.item)).sort()).toEqual([CAT, DOG]);
      expect(stored.find((c) => Number(c.item) === CAT)?.reps).toBe(3);
      expect(stored.find((c) => Number(c.item) === DOG)?.status).toBe("staging");
    });

    it("overwrites a previous backup with the current IDB contents", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ sequence: 99 })]);
      await idb.putCard(makeCard({ sequence: CAT }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(Number(stored[0].item)).toBe(CAT);
    });
  });

  describe("syncCardsBackup", () => {
    it("restores cards from storage.local when IDB is empty and a backup exists", async () => {
      const cards = [
        makeCard({ sequence: CAT, reps: 5, stability: 12, difficulty: 4.2, state: "review" }),
        makeCard({ sequence: DOG, status: "staging" }),
      ];
      storage.set(backup.CARDS_BACKUP_KEY, cards);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 2, backedUp: 0 });
      const restored = await idb.getAllCards();
      expect(restored).toHaveLength(2);
      expect(restored.find((c) => Number(c.item) === CAT)?.reps).toBe(5);
      expect(restored.find((c) => Number(c.item) === CAT)?.stability).toBe(12);
      expect(restored.find((c) => Number(c.item) === DOG)?.status).toBe("staging");
    });

    it("refreshes the backup from IDB when IDB has cards (IDB always wins)", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ sequence: 77 })]);
      await idb.putCard(makeCard({ sequence: CAT, reps: 9 }));

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(Number(stored[0].item)).toBe(CAT);
      expect(stored[0].reps).toBe(9);
      const idbCards = await idb.getAllCards();
      expect(idbCards).toHaveLength(1);
      expect(Number(idbCards[0].item)).toBe(CAT);
    });

    it("seeds the backup on first run when IDB has cards but no backup exists", async () => {
      await idb.putCard(makeCard({ sequence: CAT }));
      expect(storage.has(backup.CARDS_BACKUP_KEY)).toBe(false);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      expect(Number((storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[])[0].item)).toBe(CAT);
    });

    it("is a no-op when IDB and backup are both empty", async () => {
      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllCards()).toHaveLength(0);
    });

    it("does not crash when IDB is empty and no backup key has ever been written", async () => {
      await expect(backup.syncCardsBackup()).resolves.toEqual({ restored: 0, backedUp: 0 });
    });

    it("treats an empty-array backup as 'nothing to restore'", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, []);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllCards()).toHaveLength(0);
    });

    it("skips a legacy (pre-sequence) word-keyed backup row on restore", async () => {
      // Word-keyed cards can't be mapped to sequences — the clean break drops
      // them rather than guessing. The restore should add nothing.
      const legacy = {
        id: "猫::recognition",
        word: "猫",
        direction: "recognition",
        due_ms: 0,
        stability: 0,
        difficulty: 0,
        reps: 0,
        lapses: 0,
        state: "new",
        added_ms: 0,
        status: "active",
      } as unknown as SrsCard;
      storage.set(backup.CARDS_BACKUP_KEY, [legacy]);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllCards()).toHaveLength(0);
    });
  });

  describe("the card key change", () => {
    /** One card of each kind, and one of a kind from a newer client. */
    const deck = (): SrsCard[] => [
      makeCard({ sequence: CAT, reps: 5, stability: 12, state: "review", updated_ms: 7 }),
      makeCard({ sequence: CAT, direction: "recall", again_count: 2 }),
      makeCard({ kind: "kanji", item: "猫", reps: 4 }),
      makeCard({ kind: "grammar", item: "417", status: "staging", priority: 3 }),
      { ...makeCard(), id: "sentence:9:cloze", kind: "sentence", item: "9", direction: "cloze" } as unknown as SrsCard,
    ];

    it("removes the old snapshot and restores nothing from it", async () => {
      storage.set(OLD_CARDS_BACKUP_KEY, EXPORT_V2);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(storage.has(OLD_CARDS_BACKUP_KEY)).toBe(false);
      expect(await idb.getAllRawCards()).toEqual([]);
    });

    it("never restores an old-shape card put under the new key", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, EXPORT_V2);

      expect(await backup.syncCardsBackup()).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllRawCards()).toEqual([]);
    });

    it("restores every card of a snapshot field for field, unknown kinds too", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, deck());

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 5, backedUp: 0 });
      const restored = (await idb.getAllRawCards()).sort(byId);
      // putCard stamps every write, so a restored card's merge key moves
      // forward; every other field comes back as it was.
      const unstamped = (c: SrsCard) => ({ ...c, updated_ms: 0 });
      expect(restored.map(unstamped)).toEqual(deck().sort(byId).map(unstamped));
      // Only the four this build reads are shown.
      expect((await idb.getAllCards()).map((c) => c.id).sort()).toEqual([
        "grammar:417:recognition",
        "kanji:猫:meaning",
        "word:1001:recall",
        "word:1001:recognition",
      ]);
    });

    it("snapshots the cards this build can't read too", async () => {
      for (const c of deck()) await idb.putCard(c);

      await backup.writeCardsBackup();

      const ids = (storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[]).map((c) => c.id).sort();
      expect(ids).toEqual(deck().map((c) => c.id).sort());
    });
  });

  describe("importCards", () => {
    it("returns an error when payload is not an array", async () => {
      expect(await backup.importCards(null)).toEqual({
        added: 0,
        skipped: 0,
        error: "cards is not an array",
      });
      expect(await backup.importCards({ sequence: CAT })).toMatchObject({ error: expect.any(String) });
      expect(await backup.importCards("not an array")).toMatchObject({ error: expect.any(String) });
    });

    it("imports a kanji card as-is", async () => {
      const neko = makeCard({ kind: "kanji", item: "猫", reps: 4 });

      expect(await backup.importCards([neko])).toEqual({ added: 1, skipped: 0 });

      const all = await idb.getAllCards();
      expect(all).toHaveLength(1);
      expect(all[0]).toMatchObject({ id: "kanji:猫:meaning", direction: "meaning", reps: 4 });
    });

    it("adds every card when none exist", async () => {
      const result = await backup.importCards([
        makeCard({ sequence: CAT }),
        makeCard({ sequence: DOG }),
      ]);

      expect(result).toEqual({ added: 2, skipped: 0 });
      expect(await idb.getAllCards()).toHaveLength(2);
    });

    // Critical: importing must never overwrite a card the user is reviewing.
    it("preserves existing cards' review state when re-importing", async () => {
      await idb.putCard(makeCard({ sequence: CAT, reps: 9, stability: 30, due_ms: 12345 }));
      const importedCat = makeCard({ sequence: CAT, reps: 0, stability: 0, due_ms: 0 });

      const result = await backup.importCards([importedCat, makeCard({ sequence: DOG })]);

      expect(result).toEqual({ added: 1, skipped: 1 });
      const cat = await idb.getCard(wordKey(CAT), "recognition");
      expect(cat?.reps).toBe(9);
      expect(cat?.stability).toBe(30);
      expect(cat?.due_ms).toBe(12345);
    });

    it("skips entries not in the current shape", async () => {
      const result = await backup.importCards([
        null,
        { kind: "word", item: 1001, direction: "recognition" },
        { ...makeCard({ sequence: DOG }), id: "word:9:recognition" },
        EXPORT_V2[0],
        makeCard({ sequence: CAT }),
      ]);

      expect(result).toEqual({ added: 1, skipped: 4 });
      expect((await idb.getAllCards()).map((c) => c.id)).toEqual(["word:1001:recognition"]);
    });

    it("returns 0/0 for an empty array (no error)", async () => {
      expect(await backup.importCards([])).toEqual({ added: 0, skipped: 0 });
    });
  });
});
