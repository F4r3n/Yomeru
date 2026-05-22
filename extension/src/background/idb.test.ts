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
      await idb.putCard(makeCard({ word: "猫", direction: "recognition", reps: 3 }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall", reps: 7 }));

      expect((await idb.getCard("猫", "recognition"))?.reps).toBe(3);
      expect((await idb.getCard("猫", "recall"))?.reps).toBe(7);
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

  describe("putCards", () => {
    it("writes every card in one transaction", async () => {
      await idb.putCards([
        makeCard({ word: "猫", direction: "recognition" }),
        makeCard({ word: "猫", direction: "recall" }),
      ]);

      expect(await idb.getCardsByWord("猫")).toHaveLength(2);
    });

    it("is a no-op for an empty array", async () => {
      await expect(idb.putCards([])).resolves.toBeUndefined();
      expect(await idb.getAllCards()).toEqual([]);
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

  describe("tombstones", () => {
    // Deletes need to leave a tombstone so the next sync can propagate the
    // delete to the server / other devices. Without this, a deleted card
    // would resurrect on the next sync because the server has no way to
    // tell "missing from client" from "deleted by client".

    it("deleteCard writes a tombstone for each sibling id", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "猫", direction: "recall" }));

      await idb.deleteCard("猫");

      const tombs = await idb.getAllTombstones();
      expect(tombs.sort()).toEqual(
        [cardId("猫", "recognition"), cardId("猫", "recall")].sort(),
      );
    });

    it("deleteCardById writes exactly one tombstone", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));

      await idb.deleteCardById(cardId("猫", "recognition"));

      expect(await idb.getAllTombstones()).toEqual([
        cardId("猫", "recognition"),
      ]);
    });

    it("clearTombstones removes the given ids", async () => {
      await idb.deleteCard("猫");
      const before = await idb.getAllTombstones();
      expect(before).toHaveLength(2);

      await idb.clearTombstones(before);

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("clearTombstones is a no-op for an empty array", async () => {
      await idb.deleteCard("猫");
      await expect(idb.clearTombstones([])).resolves.toBeUndefined();
      expect(await idb.getAllTombstones()).toHaveLength(2);
    });

    it("applyRemoteDeletions removes cards without writing tombstones", async () => {
      // Server-driven deletes: we don't want a second round-trip to re-tell
      // the server about ids it just told us about.
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.putCard(makeCard({ word: "犬", direction: "recognition" }));

      await idb.applyRemoteDeletions([cardId("猫", "recognition")]);

      expect(await idb.getCard("猫", "recognition")).toBeNull();
      expect(await idb.getCard("犬", "recognition")).not.toBeNull();
      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("getAllTombstones returns an empty array on a fresh db", async () => {
      expect(await idb.getAllTombstones()).toEqual([]);
    });
  });

  describe("applySyncResponse", () => {
    // This is the merge step every sync runs: take what the server returned
    // and reconcile local IDB. Test the three effects independently so a
    // regression in one doesn't get masked by the others.

    it("upserts cards the server returned", async () => {
      const fromServer = makeCard({
        word: "本",
        direction: "recognition",
        stability: 4.2,
      });

      await idb.applySyncResponse({ cards: [fromServer], deletions: [] }, []);

      const stored = await idb.getCard("本", "recognition");
      expect(stored?.stability).toBe(4.2);
    });

    it("removes locally-stored cards listed in server deletions", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));

      await idb.applySyncResponse(
        { cards: [], deletions: [cardId("猫", "recognition")] },
        [],
      );

      expect(await idb.getCard("猫", "recognition")).toBeNull();
    });

    it("does not re-tombstone server-reported deletions (tombstone set converges)", async () => {
      // If the server tells us "X is deleted", we shouldn't write a new
      // local tombstone — we'd just send it back next sync forever.
      await idb.applySyncResponse(
        { cards: [], deletions: [cardId("猫", "recognition")] },
        [],
      );

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("clears tombstones we successfully forwarded", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      await idb.deleteCard("猫");
      const sent = await idb.getAllTombstones();
      expect(sent.length).toBeGreaterThan(0);

      // Server acks: deletions list reflects everything it knows, including
      // the ids we just sent.
      await idb.applySyncResponse({ cards: [], deletions: sent }, sent);

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("is a no-op when the server returns no changes", async () => {
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));

      await idb.applySyncResponse({ cards: [], deletions: [] }, []);

      expect(await idb.getCard("猫", "recognition")).not.toBeNull();
    });

    it("tolerates a missing 'deletions' field for backwards-compat", async () => {
      // Older servers may not send the field at all.
      const card = makeCard({ word: "本" });
      await idb.applySyncResponse({ cards: [card] }, []);
      expect(await idb.getCard("本", "recognition")).not.toBeNull();
    });

    it("survives re-add during sync (Bug 1 regression)", async () => {
      // Scenario:
      //   1. user deletes 猫 → tombstone written, card removed
      //   2. auto-sync fires, reads sentTombstones = [id]
      //   3. while POST is in flight, user re-adds 猫 → card written back
      //   4. server response arrives with deletions = [id]
      // We must NOT delete the re-added card. The id was in sentTombstones,
      // so apply_remote_deletions should skip it.
      const id = cardId("猫", "recognition");
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));
      // Simulate the re-add: tombstone gone (cleared after delete-and-readd
      // would have happened in real flow), card present.
      const sentTombstones = [id];

      await idb.applySyncResponse(
        { cards: [], deletions: [id] },
        sentTombstones,
      );

      // Card must survive.
      expect(await idb.getCard("猫", "recognition")).not.toBeNull();
    });

    it("still applies deletions that another device originated", async () => {
      // Same shape as the race test, but this time we did NOT send a
      // tombstone for the id → the server is telling us about a foreign
      // delete, which we should apply.
      const id = cardId("猫", "recognition");
      await idb.putCard(makeCard({ word: "猫", direction: "recognition" }));

      await idb.applySyncResponse({ cards: [], deletions: [id] }, []);

      expect(await idb.getCard("猫", "recognition")).toBeNull();
    });

    it("does not clobber a card with a newer local last_review_ms", async () => {
      // Scenario: sync goes out, user reviews 猫 during the round-trip
      // (newer last_review_ms locally), server returns the old version.
      // Server's older copy must NOT overwrite the freshly-reviewed local
      // card.
      await idb.putCard(
        makeCard({
          word: "猫",
          direction: "recognition",
          stability: 9.0,
          last_review_ms: 2_000,
        }),
      );
      const olderFromServer = makeCard({
        word: "猫",
        direction: "recognition",
        stability: 1.0,
        last_review_ms: 1_000,
      });

      await idb.applySyncResponse({ cards: [olderFromServer], deletions: [] }, []);

      const local = await idb.getCard("猫", "recognition");
      expect(local?.stability).toBe(9.0);
    });

    it("accepts a server card when its last_review_ms is newer", async () => {
      await idb.putCard(
        makeCard({
          word: "猫",
          direction: "recognition",
          stability: 1.0,
          last_review_ms: 1_000,
        }),
      );
      const newerFromServer = makeCard({
        word: "猫",
        direction: "recognition",
        stability: 9.0,
        last_review_ms: 2_000,
      });

      await idb.applySyncResponse({ cards: [newerFromServer], deletions: [] }, []);

      const local = await idb.getCard("猫", "recognition");
      expect(local?.stability).toBe(9.0);
    });
  });

  describe("v2 → v4 migration", () => {
    // High-risk: on upgrade we cursor-collect every card, delete the store,
    // recreate with a new keyPath, then re-insert + spawn recall siblings,
    // mapping SM-2 → FSRS fields. A regression here permanently corrupts
    // existing users' decks.
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

    it("upgrades each legacy card to FSRS-shaped recognition + recall siblings", async () => {
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

      const before = Date.now();
      const all = await idb.getAllCards();
      const after = Date.now();

      expect(all).toHaveLength(4);

      const catRecognition = all.find((c) => c.word === "猫" && c.direction === "recognition")!;
      expect(catRecognition.id).toBe(cardId("猫", "recognition"));
      expect(catRecognition.reps).toBe(3);
      // SM-2 interval_days → FSRS stability
      expect(catRecognition.stability).toBe(4);
      expect(catRecognition.difficulty).toBeGreaterThan(0);
      expect(catRecognition.state).toBe("review");
      expect(catRecognition.due_ms).toBe(100);
      expect(catRecognition.status).toBe("active");

      const catRecall = all.find((c) => c.word === "猫" && c.direction === "recall")!;
      expect(catRecall.id).toBe(cardId("猫", "recall"));
      expect(catRecall.reps).toBe(0);
      expect(catRecall.stability).toBe(0);
      expect(catRecall.state).toBe("new");
      expect(catRecall.status).toBe("active");
      expect(catRecall.due_ms).toBeGreaterThanOrEqual(before);
      expect(catRecall.due_ms).toBeLessThanOrEqual(after);

      // Staging on the original carries over.
      const dogRecognition = all.find((c) => c.word === "犬" && c.direction === "recognition")!;
      expect(dogRecognition.status).toBe("staging");
      // Repetitions=0 → state stays "new", stability stays 0
      expect(dogRecognition.state).toBe("new");
      expect(dogRecognition.stability).toBe(0);
      // Recall sibling is always spawned as active regardless of original status.
      const dogRecall = all.find((c) => c.word === "犬" && c.direction === "recall")!;
      expect(dogRecall.status).toBe("active");
    });

    it("leaves a fresh install (oldVersion = 0) with an empty cards store", async () => {
      expect(await idb.getAllCards()).toEqual([]);
    });
  });

  describe("v3 → v4 migration", () => {
    // Users already on v3 have composite-id sibling pairs with SM-2 fields.
    // The v3→v4 step walks each card and rewrites to FSRS-shaped fields in place.
    function openV3(): Promise<IDBDatabase> {
      return new Promise((resolve, reject) => {
        const req = indexedDB.open("yomeru-db", 3);
        req.onupgradeneeded = (e) => {
          const db = (e.target as IDBOpenDBRequest).result;
          const cards = db.createObjectStore("cards", { keyPath: "id" });
          cards.createIndex("due_ms", "due_ms", { unique: false });
          cards.createIndex("added_ms", "added_ms", { unique: false });
          cards.createIndex("status", "status", { unique: false });
          cards.createIndex("word", "word", { unique: false });
          db.createObjectStore("lookup_history", {
            keyPath: "id",
            autoIncrement: true,
          });
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      });
    }

    async function seedV3(db: IDBDatabase, cards: Array<Record<string, unknown>>) {
      await new Promise<void>((resolve, reject) => {
        const t = db.transaction("cards", "readwrite");
        const store = t.objectStore("cards");
        for (const c of cards) store.add(c);
        t.oncomplete = () => resolve();
        t.onerror = () => reject(t.error);
      });
    }

    it("rewrites SM-2 fields to FSRS fields on every existing card", async () => {
      const v3 = await openV3();
      await seedV3(v3, [
        {
          id: cardId("猫", "recognition"),
          word: "猫",
          direction: "recognition",
          due_ms: 100,
          interval_days: 6,
          ease_factor: 2.5,
          repetitions: 2,
          added_ms: 50,
          status: "active",
        },
        {
          id: cardId("猫", "recall"),
          word: "猫",
          direction: "recall",
          due_ms: 100,
          interval_days: 0,
          ease_factor: 2.5,
          repetitions: 0,
          added_ms: 50,
          status: "active",
        },
      ]);
      v3.close();

      const all = await idb.getAllCards();
      expect(all).toHaveLength(2);

      const recognition = all.find((c) => c.direction === "recognition")!;
      expect(recognition.stability).toBe(6);
      expect(recognition.reps).toBe(2);
      expect(recognition.state).toBe("review");
      expect(recognition.due_ms).toBe(100);

      const recall = all.find((c) => c.direction === "recall")!;
      expect(recall.stability).toBe(0);
      expect(recall.reps).toBe(0);
      expect(recall.state).toBe("new");
    });
  });
});
