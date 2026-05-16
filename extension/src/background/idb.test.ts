import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");

function makeCard(overrides: Partial<SrsCard> = {}): SrsCard {
  const word = overrides.word ?? "食べる";
  const direction = overrides.direction ?? "recognition";
  return {
    id: cardId(word, direction),
    word,
    direction,
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
    it("promotes both direction siblings of a word from staging to active", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall", status: "staging" }));

      await idb.promoteCard("猫");

      expect((await idb.getCard("猫", "recognition"))?.status).toBe("active");
      expect((await idb.getCard("猫", "recall"))?.status).toBe("active");
    });

    it("is a no-op for a non-existent word", async () => {
      await expect(idb.promoteCard("存在しない")).resolves.toBeUndefined();
    });

    it("does not affect other words", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition", status: "staging" }));

      await idb.promoteCard("猫");

      expect((await idb.getCard("犬", "recognition"))?.status).toBe("staging");
    });
  });

  describe("promoteAll", () => {
    it("sets all staging cards to active", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition", status: "staging" }));

      await idb.promoteAll();

      expect(await idb.getStagingCards()).toHaveLength(0);
      const all = await idb.getAllCards();
      expect(all.every((c) => c.status === "active")).toBe(true);
    });

    it("does not affect already active cards", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", status: "active", due_ms: 999 }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition", status: "staging" }));

      await idb.promoteAll();

      const cat = await idb.getCard("猫", "recognition");
      expect(cat?.status).toBe("active");
      expect(cat?.due_ms).toBe(999);
    });

    it("is a no-op when no staging cards exist", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", status: "active" }));

      await expect(idb.promoteAll()).resolves.toBeUndefined();
      expect((await idb.getCard("猫", "recognition"))?.status).toBe("active");
    });
  });

  describe("getCard", () => {
    it("returns only the requested direction sibling", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", repetitions: 3 }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall", repetitions: 7 }));

      expect((await idb.getCard("猫", "recognition"))?.repetitions).toBe(3);
      expect((await idb.getCard("猫", "recall"))?.repetitions).toBe(7);
    });

    it("returns null when the direction is missing even if the other exists", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));

      expect(await idb.getCard("猫", "recall")).toBeNull();
    });
  });

  describe("getCardsByWord", () => {
    it("returns both direction siblings", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition" }));

      const siblings = await idb.getCardsByWord("猫");

      expect(siblings).toHaveLength(2);
      expect(siblings.map((c) => c.direction).sort()).toEqual(["recall", "recognition"]);
    });

    it("returns an empty array for an unknown word", async () => {
      expect(await idb.getCardsByWord("存在しない")).toEqual([]);
    });
  });

  describe("deleteCard", () => {
    it("removes both direction siblings for the word", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall" }));

      await idb.deleteCard("猫");

      expect(await idb.getCardsByWord("猫")).toEqual([]);
    });

    it("does not affect other words' siblings", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recall" }));

      await idb.deleteCard("猫");

      expect((await idb.getCardsByWord("犬"))).toHaveLength(2);
    });
  });

  describe("deleteCardById", () => {
    // Used when one direction graduates: the sibling must remain reviewable.
    it("removes one sibling without touching the other", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall" }));

      await idb.deleteCardById(cardId("猫", "recognition"));

      expect(await idb.getCard("猫", "recognition")).toBeNull();
      expect(await idb.getCard("猫", "recall")).not.toBeNull();
    });
  });

  describe("v2 → v3 migration", () => {
    // High-risk: on upgrade we cursor-collect every card, delete the store,
    // recreate with a new keyPath, then re-insert + spawn recall siblings.
    // A regression here permanently corrupts existing users' decks.
    function openV2(): Promise<IDBDatabase> {
      return new Promise((resolve, reject) => {
        const req = indexedDB.open("yomeru-db", 2);
        req.onupgradeneeded = (e) => {
          const db = (e.target as IDBOpenDBRequest).result;
          const cards = db.createObjectStore("cards", { keyPath: "word" });
          cards.createIndex("due_ms", "due_ms", { unique: false });
          cards.createIndex("added_ms", "added_ms", { unique: false });
          cards.createIndex("status", "status", { unique: false });
          db.createObjectStore("lookup_history", {
            keyPath: "id",
            autoIncrement: true,
          });
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      });
    }

    async function seedV2(db: IDBDatabase, cards: Array<Record<string, unknown>>) {
      await new Promise<void>((resolve, reject) => {
        const t = db.transaction("cards", "readwrite");
        const store = t.objectStore("cards");
        for (const c of cards) store.add(c);
        t.oncomplete = () => resolve();
        t.onerror = () => reject(t.error);
      });
    }

    it("upgrades each legacy card into a recognition sibling and spawns a recall sibling", async () => {
      const v2 = await openV2();
      await seedV2(v2, [
        {
          word: "猫",
          due_ms: 100,
          interval_days: 4,
          ease_factor: 2.6,
          repetitions: 3,
          added_ms: 50,
          status: "active",
        },
        {
          word: "犬",
          due_ms: 200,
          interval_days: 1,
          ease_factor: 2.5,
          repetitions: 0,
          added_ms: 60,
          status: "staging",
        },
      ]);
      v2.close();

      // Triggers v2→v3 migration via the real openDb path.
      const before = Date.now();
      const all = await idb.getAllCards();
      const after = Date.now();

      expect(all).toHaveLength(4);

      const catRecognition = all.find((c) => c.word === "猫" && c.direction === "recognition")!;
      expect(catRecognition.id).toBe(cardId("猫", "recognition"));
      expect(catRecognition.repetitions).toBe(3);
      expect(catRecognition.interval_days).toBe(4);
      expect(catRecognition.ease_factor).toBeCloseTo(2.6, 5);
      expect(catRecognition.due_ms).toBe(100);
      expect(catRecognition.status).toBe("active");

      const catRecall = all.find((c) => c.word === "猫" && c.direction === "recall")!;
      expect(catRecall.id).toBe(cardId("猫", "recall"));
      expect(catRecall.repetitions).toBe(0);
      expect(catRecall.interval_days).toBe(0);
      expect(catRecall.ease_factor).toBe(2.5);
      expect(catRecall.status).toBe("active");
      // The recall sibling becomes due immediately.
      expect(catRecall.due_ms).toBeGreaterThanOrEqual(before);
      expect(catRecall.due_ms).toBeLessThanOrEqual(after);

      // Staging on the original carries over.
      const dogRecognition = all.find((c) => c.word === "犬" && c.direction === "recognition")!;
      expect(dogRecognition.status).toBe("staging");
      // Recall sibling is always spawned as active regardless of original status.
      const dogRecall = all.find((c) => c.word === "犬" && c.direction === "recall")!;
      expect(dogRecall.status).toBe("active");
    });

    it("leaves a fresh install (oldVersion = 0) with an empty cards store", async () => {
      // No pre-seeded v2 DB — just open via the v3 path directly.
      expect(await idb.getAllCards()).toEqual([]);
    });
  });
});
