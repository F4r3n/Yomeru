import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");

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

describe("idb", () => {
  let idb: IdbModule;

  beforeEach(async () => {
    vi.resetModules();
    globalThis.indexedDB = new IDBFactory();
    (globalThis as unknown as Record<string, unknown>).IDBKeyRange = FakeIDBKeyRange;
    idb = await import("./idb.ts");
  });

  describe("getDueCards", () => {
    it("returns active cards with due_ms <= now", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ word: "猫", due_ms: now - 1000, status: "active" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(1);
      expect(due[0].word).toBe("猫");
    });

    it("excludes staging cards even when overdue", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ word: "猫", due_ms: now - 1000, status: "staging" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(0);
    });

    it("excludes active cards not yet due", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ word: "猫", due_ms: now + 86_400_000, status: "active" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(0);
    });

    it("returns multiple due active cards", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ word: "猫", due_ms: now - 2000, status: "active" }));
      await idb.putCard(makeCard({ word: "犬", due_ms: now - 1000, status: "active" }));
      await idb.putCard(makeCard({ word: "鳥", due_ms: now - 500, status: "staging" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(2);
      expect(due.map((c) => c.word)).toEqual(expect.arrayContaining(["猫", "犬"]));
    });
  });

  describe("getStagingCards", () => {
    it("returns only staging cards", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "staging" }));
      await idb.putCard(makeCard({ word: "犬", status: "active" }));

      const staging = await idb.getStagingCards();

      expect(staging).toHaveLength(1);
      expect(staging[0].word).toBe("猫");
    });

    it("returns empty array when no staging cards exist", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "active" }));

      expect(await idb.getStagingCards()).toHaveLength(0);
    });
  });

  describe("promoteCard", () => {
    it("sets a staging card to active", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "staging" }));

      await idb.promoteCard("猫");

      const card = await idb.getCard("猫");
      expect(card?.status).toBe("active");
    });

    it("is a no-op for a non-existent word", async () => {
      await expect(idb.promoteCard("存在しない")).resolves.toBeUndefined();
    });

    it("does not affect other cards", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "staging" }));
      await idb.putCard(makeCard({ word: "犬", status: "staging" }));

      await idb.promoteCard("猫");

      expect((await idb.getCard("犬"))?.status).toBe("staging");
    });
  });

  describe("promoteAll", () => {
    it("sets all staging cards to active", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "staging" }));
      await idb.putCard(makeCard({ word: "犬", status: "staging" }));

      await idb.promoteAll();

      expect(await idb.getStagingCards()).toHaveLength(0);
      const all = await idb.getAllCards();
      expect(all.every((c) => c.status === "active")).toBe(true);
    });

    it("does not affect already active cards", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "active", due_ms: 999 }));
      await idb.putCard(makeCard({ word: "犬", status: "staging" }));

      await idb.promoteAll();

      const cat = await idb.getCard("猫");
      expect(cat?.status).toBe("active");
      expect(cat?.due_ms).toBe(999);
    });

    it("is a no-op when no staging cards exist", async () => {
      await idb.putCard(makeCard({ word: "猫", status: "active" }));

      await expect(idb.promoteAll()).resolves.toBeUndefined();
      expect((await idb.getCard("猫"))?.status).toBe("active");
    });
  });
});
