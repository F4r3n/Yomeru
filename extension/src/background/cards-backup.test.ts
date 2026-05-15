import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");
type CardsBackupModule = typeof import("./cards-backup.ts");

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  return {
    word: "食べる",
    reading: "たべる",
    meaning_en: "to eat",
    due_ms: 0,
    interval_days: 1,
    ease_factor: 2.5,
    repetitions: 0,
    added_ms: 0,
    status: "active",
    ...overrides,
  };
}

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
        },
      },
    });
    // Silence the restore-warning in tests so output stays clean.
    vi.spyOn(console, "warn").mockImplementation(() => {});
    idb = await import("./idb.ts");
    backup = await import("./cards-backup.ts");
  });

  describe("writeCardsBackup", () => {
    it("writes an empty array when IDB has no cards", async () => {
      await backup.writeCardsBackup();
      expect(storage.get(backup.CARDS_BACKUP_KEY)).toEqual([]);
    });

    it("snapshots every IDB card into storage.local", async () => {
      await idb.putCard(makeCard({ word: "猫", repetitions: 3 }));
      await idb.putCard(makeCard({ word: "犬", status: "staging" }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(2);
      expect(stored.map((c) => c.word).sort()).toEqual(["犬", "猫"]);
      expect(stored.find((c) => c.word === "猫")?.repetitions).toBe(3);
      expect(stored.find((c) => c.word === "犬")?.status).toBe("staging");
    });

    it("overwrites a previous backup with the current IDB contents", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ word: "古い" })]);
      await idb.putCard(makeCard({ word: "新しい" }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(stored[0].word).toBe("新しい");
    });
  });

  describe("syncCardsBackup", () => {
    // The disaster-recovery path: this is the regression that motivated the
    // mirror — an extension reinstall wiped IDB silently. If this test ever
    // fails, users will lose their decks again on the next install hiccup.
    it("restores cards from storage.local when IDB is empty and a backup exists", async () => {
      const cards = [
        makeCard({ word: "猫", repetitions: 5, ease_factor: 2.7 }),
        makeCard({ word: "犬", status: "staging" }),
      ];
      storage.set(backup.CARDS_BACKUP_KEY, cards);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 2, backedUp: 0 });
      const restored = await idb.getAllCards();
      expect(restored).toHaveLength(2);
      expect(restored.find((c) => c.word === "猫")?.repetitions).toBe(5);
      expect(restored.find((c) => c.word === "猫")?.ease_factor).toBe(2.7);
      expect(restored.find((c) => c.word === "犬")?.status).toBe("staging");
    });

    it("refreshes the backup from IDB when IDB has cards (IDB always wins)", async () => {
      // A stale/wrong backup must never overwrite a live IDB.
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ word: "stale" })]);
      await idb.putCard(makeCard({ word: "猫", repetitions: 9 }));

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(stored[0].word).toBe("猫");
      expect(stored[0].repetitions).toBe(9);
      // IDB itself untouched.
      const idbCards = await idb.getAllCards();
      expect(idbCards).toHaveLength(1);
      expect(idbCards[0].word).toBe("猫");
    });

    it("seeds the backup on first run when IDB has cards but no backup exists", async () => {
      await idb.putCard(makeCard({ word: "猫" }));
      expect(storage.has(backup.CARDS_BACKUP_KEY)).toBe(false);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      expect((storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[])[0].word).toBe("猫");
    });

    it("is a no-op when IDB and backup are both empty", async () => {
      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllCards()).toHaveLength(0);
    });

    it("does not crash when IDB is empty and no backup key has ever been written", async () => {
      // Fresh install: storage.local has never seen our key.
      await expect(backup.syncCardsBackup()).resolves.toEqual({ restored: 0, backedUp: 0 });
    });

    it("treats an empty-array backup as 'nothing to restore'", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, []);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 0 });
      expect(await idb.getAllCards()).toHaveLength(0);
    });
  });

  describe("importCards", () => {
    it("returns an error when payload is not an array", async () => {
      expect(await backup.importCards(null)).toEqual({
        added: 0,
        skipped: 0,
        error: "cards is not an array",
      });
      expect(await backup.importCards({ word: "猫" })).toMatchObject({ error: expect.any(String) });
      expect(await backup.importCards("not an array")).toMatchObject({ error: expect.any(String) });
    });

    it("adds every card when none exist", async () => {
      const result = await backup.importCards([
        makeCard({ word: "猫" }),
        makeCard({ word: "犬" }),
      ]);

      expect(result).toEqual({ added: 2, skipped: 0 });
      expect(await idb.getAllCards()).toHaveLength(2);
    });

    // Critical: importing must never overwrite a card the user is reviewing.
    // If this regresses, a user who imports a backup will lose review progress
    // on cards they already had.
    it("preserves existing cards' review state when re-importing", async () => {
      await idb.putCard(makeCard({ word: "猫", repetitions: 9, ease_factor: 2.9, due_ms: 12345 }));
      const importedCat = makeCard({ word: "猫", repetitions: 0, ease_factor: 2.5, due_ms: 0 });

      const result = await backup.importCards([importedCat, makeCard({ word: "犬" })]);

      expect(result).toEqual({ added: 1, skipped: 1 });
      const cat = await idb.getCard("猫");
      expect(cat?.repetitions).toBe(9);
      expect(cat?.ease_factor).toBe(2.9);
      expect(cat?.due_ms).toBe(12345);
    });

    it("skips entries missing a string `word`", async () => {
      const result = await backup.importCards([
        null,
        { word: 123 },
        { reading: "no word here" },
        makeCard({ word: "猫" }),
      ]);

      expect(result).toEqual({ added: 1, skipped: 3 });
      expect((await idb.getAllCards())[0].word).toBe("猫");
    });

    it("returns 0/0 for an empty array (no error)", async () => {
      expect(await backup.importCards([])).toEqual({ added: 0, skipped: 0 });
    });
  });
});
