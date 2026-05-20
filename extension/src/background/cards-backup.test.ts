import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");
type CardsBackupModule = typeof import("./cards-backup.ts");

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  const word = overrides.word ?? "食べる";
  const direction = overrides.direction ?? "recognition";
  return {
    id: cardId(word, direction),
    word,
    direction,
    due_ms: 0,
    stability: 0,
    difficulty: 0,
    reps: 0,
    lapses: 0,
    state: "new",
    last_review_ms: null,
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
    vi.spyOn(console, "warn").mockImplementation(() => {});
    idb = await import("./idb.ts");
    backup = await import("./cards-backup.ts");
  });

  describe("writeCardsBackup", () => {
    it("does not overwrite a backup when IDB is empty", async () => {
      const existing = [makeCard({ word: "previously-backed-up" })];
      storage.set(backup.CARDS_BACKUP_KEY, existing);

      await backup.writeCardsBackup();

      expect(storage.get(backup.CARDS_BACKUP_KEY)).toEqual(existing);
    });

    it("does not create a backup when IDB is empty and none exists", async () => {
      await backup.writeCardsBackup();
      expect(storage.has(backup.CARDS_BACKUP_KEY)).toBe(false);
    });

    it("snapshots every IDB card into storage.local", async () => {
      await idb.putCard(makeCard({ word: "猫", reps: 3 }));
      await idb.putCard(makeCard({ word: "犬", status: "staging" }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(2);
      expect(stored.map((c) => c.word).sort()).toEqual(["犬", "猫"]);
      expect(stored.find((c) => c.word === "猫")?.reps).toBe(3);
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
    it("restores cards from storage.local when IDB is empty and a backup exists", async () => {
      const cards = [
        makeCard({ word: "猫", reps: 5, stability: 12, difficulty: 4.2, state: "review" }),
        makeCard({ word: "犬", status: "staging" }),
      ];
      storage.set(backup.CARDS_BACKUP_KEY, cards);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 2, backedUp: 0 });
      const restored = await idb.getAllCards();
      expect(restored).toHaveLength(2);
      expect(restored.find((c) => c.word === "猫")?.reps).toBe(5);
      expect(restored.find((c) => c.word === "猫")?.stability).toBe(12);
      expect(restored.find((c) => c.word === "犬")?.status).toBe("staging");
    });

    it("refreshes the backup from IDB when IDB has cards (IDB always wins)", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ word: "stale" })]);
      await idb.putCard(makeCard({ word: "猫", reps: 9 }));

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(stored[0].word).toBe("猫");
      expect(stored[0].reps).toBe(9);
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
    it("preserves existing cards' review state when re-importing", async () => {
      await idb.putCard(makeCard({ word: "猫", reps: 9, stability: 30, due_ms: 12345 }));
      const importedCat = makeCard({ word: "猫", reps: 0, stability: 0, due_ms: 0 });

      const result = await backup.importCards([importedCat, makeCard({ word: "犬" })]);

      expect(result).toEqual({ added: 1, skipped: 1 });
      const cat = await idb.getCard("猫", "recognition");
      expect(cat?.reps).toBe(9);
      expect(cat?.stability).toBe(30);
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

    // The v2 JSON export shape: rows have `word` + SM-2 fields, no `id` or
    // `direction`. Import must produce a recognition sibling preserving SM-2
    // state mapped to FSRS, AND a fresh recall sibling.
    it("expands a legacy (pre-v3) JSON row into FSRS-shaped recognition + recall siblings", async () => {
      const legacy = {
        word: "猫",
        due_ms: 100,
        interval_days: 4,
        ease_factor: 2.7,
        repetitions: 3,
        added_ms: 50,
        status: "active",
      } as unknown as SrsCard;

      const before = Date.now();
      const result = await backup.importCards([legacy]);
      const after = Date.now();

      expect(result).toEqual({ added: 1, skipped: 0 });

      const recognition = await idb.getCard("猫", "recognition");
      expect(recognition?.reps).toBe(3);
      expect(recognition?.stability).toBe(4);
      expect(recognition?.state).toBe("review");
      expect(recognition?.due_ms).toBe(100);

      const recall = await idb.getCard("猫", "recall");
      expect(recall?.reps).toBe(0);
      expect(recall?.stability).toBe(0);
      expect(recall?.state).toBe("new");
      expect(recall?.status).toBe("active");
      expect(recall?.due_ms).toBeGreaterThanOrEqual(before);
      expect(recall?.due_ms).toBeLessThanOrEqual(after);
    });

    it("expands a v3 JSON row (direction + SM-2 fields) into FSRS shape, keeping that one direction", async () => {
      const v3Row = {
        id: cardId("猫", "recognition"),
        word: "猫",
        direction: "recognition" as const,
        due_ms: 100,
        interval_days: 6,
        ease_factor: 2.5,
        repetitions: 2,
        added_ms: 50,
        status: "active" as const,
      } as unknown as SrsCard;

      const result = await backup.importCards([v3Row]);

      expect(result).toEqual({ added: 1, skipped: 0 });
      const recognition = await idb.getCard("猫", "recognition");
      expect(recognition?.reps).toBe(2);
      expect(recognition?.stability).toBe(6);
      expect(recognition?.state).toBe("review");
      // No recall sibling spawned for a v3 row — only the v2 legacy path spawns one.
      expect(await idb.getCard("猫", "recall")).toBeNull();
    });

    it("does not clobber an existing recognition card when re-importing a legacy row", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", reps: 9, stability: 30 }));
      const legacy = {
        word: "猫",
        due_ms: 0,
        interval_days: 1,
        ease_factor: 2.5,
        repetitions: 0,
        added_ms: 0,
        status: "active",
      } as unknown as SrsCard;

      const result = await backup.importCards([legacy]);

      // The recall sibling didn't exist yet, so it gets added.
      expect(result).toEqual({ added: 1, skipped: 0 });
      const cat = await idb.getCard("猫", "recognition");
      expect(cat?.reps).toBe(9);
      expect(cat?.stability).toBe(30);
      expect(await idb.getCard("猫", "recall")).not.toBeNull();
    });
  });

  describe("syncCardsBackup with legacy backup", () => {
    it("restores a legacy storage.local backup as FSRS-shaped sibling pairs", async () => {
      const legacy = {
        word: "猫",
        due_ms: 100,
        interval_days: 4,
        ease_factor: 2.7,
        repetitions: 3,
        added_ms: 50,
        status: "active",
      } as unknown as SrsCard;
      storage.set(backup.CARDS_BACKUP_KEY, [legacy]);

      const result = await backup.syncCardsBackup();

      expect(result.backedUp).toBe(0);
      expect(result.restored).toBe(2);
      const recognition = await idb.getCard("猫", "recognition");
      expect(recognition?.reps).toBe(3);
      expect(recognition?.stability).toBe(4);
      expect(recognition?.state).toBe("review");
      expect((await idb.getCard("猫", "recall"))?.status).toBe("active");
    });
  });
});
