import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");
type CardsBackupModule = typeof import("./cards-backup.ts");

const CAT = 1_001;
const DOG = 1_002;

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  const sequence = overrides.sequence ?? 1_358_280;
  const direction = overrides.direction ?? "recognition";
  return {
    id: cardId(sequence, direction),
    sequence,
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
      expect(stored.map((c) => c.sequence).sort()).toEqual([CAT, DOG]);
      expect(stored.find((c) => c.sequence === CAT)?.reps).toBe(3);
      expect(stored.find((c) => c.sequence === DOG)?.status).toBe("staging");
    });

    it("overwrites a previous backup with the current IDB contents", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ sequence: 99 })]);
      await idb.putCard(makeCard({ sequence: CAT }));

      await backup.writeCardsBackup();

      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(stored[0].sequence).toBe(CAT);
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
      expect(restored.find((c) => c.sequence === CAT)?.reps).toBe(5);
      expect(restored.find((c) => c.sequence === CAT)?.stability).toBe(12);
      expect(restored.find((c) => c.sequence === DOG)?.status).toBe("staging");
    });

    it("refreshes the backup from IDB when IDB has cards (IDB always wins)", async () => {
      storage.set(backup.CARDS_BACKUP_KEY, [makeCard({ sequence: 77 })]);
      await idb.putCard(makeCard({ sequence: CAT, reps: 9 }));

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      const stored = storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[];
      expect(stored).toHaveLength(1);
      expect(stored[0].sequence).toBe(CAT);
      expect(stored[0].reps).toBe(9);
      const idbCards = await idb.getAllCards();
      expect(idbCards).toHaveLength(1);
      expect(idbCards[0].sequence).toBe(CAT);
    });

    it("seeds the backup on first run when IDB has cards but no backup exists", async () => {
      await idb.putCard(makeCard({ sequence: CAT }));
      expect(storage.has(backup.CARDS_BACKUP_KEY)).toBe(false);

      const result = await backup.syncCardsBackup();

      expect(result).toEqual({ restored: 0, backedUp: 1 });
      expect((storage.get(backup.CARDS_BACKUP_KEY) as SrsCard[])[0].sequence).toBe(CAT);
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
      const cat = await idb.getCard(CAT, "recognition");
      expect(cat?.reps).toBe(9);
      expect(cat?.stability).toBe(30);
      expect(cat?.due_ms).toBe(12345);
    });

    it("skips entries missing a numeric `sequence`", async () => {
      const result = await backup.importCards([
        null,
        { sequence: "nope" },
        { word: "猫" },
        makeCard({ sequence: CAT }),
      ]);

      expect(result).toEqual({ added: 1, skipped: 3 });
      expect((await idb.getAllCards())[0].sequence).toBe(CAT);
    });

    it("returns 0/0 for an empty array (no error)", async () => {
      expect(await backup.importCards([])).toEqual({ added: 0, skipped: 0 });
    });

    // An export row without a `direction` is expanded into both siblings,
    // preserving the row's scheduling state on the recognition sibling and
    // spawning a fresh recall sibling.
    it("expands a directionless row into recognition + recall siblings", async () => {
      const row = {
        sequence: CAT,
        due_ms: 100,
        stability: 4,
        difficulty: 5,
        reps: 3,
        lapses: 0,
        state: "review",
        last_review_ms: null,
        added_ms: 50,
        status: "active",
      } as unknown as SrsCard;

      const before = Date.now();
      const result = await backup.importCards([row]);
      const after = Date.now();

      expect(result).toEqual({ added: 1, skipped: 0 });

      const recognition = await idb.getCard(CAT, "recognition");
      expect(recognition?.reps).toBe(3);
      expect(recognition?.stability).toBe(4);
      expect(recognition?.state).toBe("review");
      expect(recognition?.due_ms).toBe(100);

      const recall = await idb.getCard(CAT, "recall");
      expect(recall?.reps).toBe(0);
      expect(recall?.stability).toBe(0);
      expect(recall?.state).toBe("new");
      expect(recall?.status).toBe("active");
      expect(recall?.due_ms).toBeGreaterThanOrEqual(before);
      expect(recall?.due_ms).toBeLessThanOrEqual(after);
    });

    it("imports a single direction sibling for a row that carries a direction", async () => {
      const row = makeCard({ sequence: CAT, direction: "recognition", reps: 2, stability: 6, state: "review" });

      const result = await backup.importCards([row]);

      expect(result).toEqual({ added: 1, skipped: 0 });
      const recognition = await idb.getCard(CAT, "recognition");
      expect(recognition?.reps).toBe(2);
      expect(recognition?.stability).toBe(6);
      expect(recognition?.state).toBe("review");
      // No recall sibling spawned — the row named its direction.
      expect(await idb.getCard(CAT, "recall")).toBeNull();
    });

    it("does not clobber an existing recognition card when re-importing a directionless row", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", reps: 9, stability: 30 }));
      const row = {
        sequence: CAT,
        due_ms: 0,
        stability: 0,
        difficulty: 0,
        reps: 0,
        lapses: 0,
        state: "new",
        last_review_ms: null,
        added_ms: 0,
        status: "active",
      } as unknown as SrsCard;

      const result = await backup.importCards([row]);

      // The recall sibling didn't exist yet, so it gets added.
      expect(result).toEqual({ added: 1, skipped: 0 });
      const cat = await idb.getCard(CAT, "recognition");
      expect(cat?.reps).toBe(9);
      expect(cat?.stability).toBe(30);
      expect(await idb.getCard(CAT, "recall")).not.toBeNull();
    });
  });
});
