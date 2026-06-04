import { beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";

type IdbModule = typeof import("./idb.ts");

// Cards key on JMdict ent_seq; the exact numbers are arbitrary, only their
// distinctness matters for these tests.
const CAT = 1_001;
const DOG = 1_002;
const BIRD = 1_003;
const BOOK = 1_004;

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
      await idb.putCard(makeCard({ sequence: CAT, due_ms: now - 1000, status: "active" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(1);
      expect(due[0].sequence).toBe(CAT);
    });

    it("excludes staging cards even when overdue", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ sequence: CAT, due_ms: now - 1000, status: "staging" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(0);
    });

    it("excludes active cards not yet due", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ sequence: CAT, due_ms: now + 86_400_000, status: "active" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(0);
    });

    it("returns multiple due active cards", async () => {
      const now = Date.now();
      await idb.putCard(makeCard({ sequence: CAT, due_ms: now - 2000, status: "active" }));
      await idb.putCard(makeCard({ sequence: DOG, due_ms: now - 1000, status: "active" }));
      await idb.putCard(makeCard({ sequence: BIRD, due_ms: now - 500, status: "staging" }));

      const due = await idb.getDueCards(now);

      expect(due).toHaveLength(2);
      expect(due.map((c) => c.sequence)).toEqual(expect.arrayContaining([CAT, DOG]));
    });
  });

  describe("getStagingCards", () => {
    it("returns only staging cards", async () => {
      await idb.putCard(makeCard({ sequence: CAT, status: "staging" }));
      await idb.putCard(makeCard({ sequence: DOG, status: "active" }));

      const staging = await idb.getStagingCards();

      expect(staging).toHaveLength(1);
      expect(staging[0].sequence).toBe(CAT);
    });

    it("returns empty array when no staging cards exist", async () => {
      await idb.putCard(makeCard({ sequence: CAT, status: "active" }));

      expect(await idb.getStagingCards()).toHaveLength(0);
    });
  });

  describe("promoteCard", () => {
    it("promotes both direction siblings of an entry from staging to active", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall", status: "staging" }));

      await idb.promoteCard(CAT);

      expect((await idb.getCard(CAT, "recognition"))?.status).toBe("active");
      expect((await idb.getCard(CAT, "recall"))?.status).toBe("active");
    });

    it("is a no-op for a non-existent sequence", async () => {
      await expect(idb.promoteCard(999_999)).resolves.toBeUndefined();
    });

    it("does not affect other entries", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition", status: "staging" }));

      await idb.promoteCard(CAT);

      expect((await idb.getCard(DOG, "recognition"))?.status).toBe("staging");
    });
  });

  describe("promoteAll", () => {
    it("sets all staging cards to active", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", status: "staging" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition", status: "staging" }));

      await idb.promoteAll();

      expect(await idb.getStagingCards()).toHaveLength(0);
      const all = await idb.getAllCards();
      expect(all.every((c) => c.status === "active")).toBe(true);
    });

    it("does not affect already active cards", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", status: "active", due_ms: 999 }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition", status: "staging" }));

      await idb.promoteAll();

      const cat = await idb.getCard(CAT, "recognition");
      expect(cat?.status).toBe("active");
      expect(cat?.due_ms).toBe(999);
    });

    it("is a no-op when no staging cards exist", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", status: "active" }));

      await expect(idb.promoteAll()).resolves.toBeUndefined();
      expect((await idb.getCard(CAT, "recognition"))?.status).toBe("active");
    });
  });

  describe("getCard", () => {
    it("returns only the requested direction sibling", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition", reps: 3 }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall", reps: 7 }));

      expect((await idb.getCard(CAT, "recognition"))?.reps).toBe(3);
      expect((await idb.getCard(CAT, "recall"))?.reps).toBe(7);
    });

    it("returns null when the direction is missing even if the other exists", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));

      expect(await idb.getCard(CAT, "recall")).toBeNull();
    });
  });

  describe("getCardsBySequence", () => {
    it("returns both direction siblings", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition" }));

      const siblings = await idb.getCardsBySequence(CAT);

      expect(siblings).toHaveLength(2);
      expect(siblings.map((c) => c.direction).sort()).toEqual(["recall", "recognition"]);
    });

    it("returns an empty array for an unknown sequence", async () => {
      expect(await idb.getCardsBySequence(999_999)).toEqual([]);
    });
  });

  describe("deleteCard", () => {
    it("removes both direction siblings for the entry", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall" }));

      await idb.deleteCard(CAT);

      expect(await idb.getCardsBySequence(CAT)).toEqual([]);
    });

    it("does not affect other entries' siblings", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recall" }));

      await idb.deleteCard(CAT);

      expect((await idb.getCardsBySequence(DOG))).toHaveLength(2);
    });
  });

  describe("putCards", () => {
    it("writes every card in one transaction", async () => {
      await idb.putCards([
        makeCard({ sequence: CAT, direction: "recognition" }),
        makeCard({ sequence: CAT, direction: "recall" }),
      ]);

      expect(await idb.getCardsBySequence(CAT)).toHaveLength(2);
    });

    it("is a no-op for an empty array", async () => {
      await expect(idb.putCards([])).resolves.toBeUndefined();
      expect(await idb.getAllCards()).toEqual([]);
    });
  });

  describe("deleteCardById", () => {
    // Used when one direction graduates: the sibling must remain reviewable.
    it("removes one sibling without touching the other", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall" }));

      await idb.deleteCardById(cardId(CAT, "recognition"));

      expect(await idb.getCard(CAT, "recognition")).toBeNull();
      expect(await idb.getCard(CAT, "recall")).not.toBeNull();
    });
  });

  describe("tombstones", () => {
    // Deletes need to leave a tombstone so the next sync can propagate the
    // delete to the server / other devices. Without this, a deleted card
    // would resurrect on the next sync because the server has no way to
    // tell "missing from client" from "deleted by client".

    it("deleteCard writes a tombstone for each sibling id", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: CAT, direction: "recall" }));

      await idb.deleteCard(CAT);

      const tombs = await idb.getAllTombstones();
      expect(tombs.sort()).toEqual(
        [cardId(CAT, "recognition"), cardId(CAT, "recall")].sort(),
      );
    });

    it("deleteCardById writes exactly one tombstone", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));

      await idb.deleteCardById(cardId(CAT, "recognition"));

      expect(await idb.getAllTombstones()).toEqual([
        cardId(CAT, "recognition"),
      ]);
    });

    it("clearTombstones removes the given ids", async () => {
      await idb.deleteCard(CAT);
      const before = await idb.getAllTombstones();
      expect(before).toHaveLength(2);

      await idb.clearTombstones(before);

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("clearTombstones is a no-op for an empty array", async () => {
      await idb.deleteCard(CAT);
      await expect(idb.clearTombstones([])).resolves.toBeUndefined();
      expect(await idb.getAllTombstones()).toHaveLength(2);
    });

    it("applyRemoteDeletions removes cards without writing tombstones", async () => {
      // Server-driven deletes: we don't want a second round-trip to re-tell
      // the server about ids it just told us about.
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.putCard(makeCard({ sequence: DOG, direction: "recognition" }));

      await idb.applyRemoteDeletions([cardId(CAT, "recognition")]);

      expect(await idb.getCard(CAT, "recognition")).toBeNull();
      expect(await idb.getCard(DOG, "recognition")).not.toBeNull();
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
        sequence: BOOK,
        direction: "recognition",
        stability: 4.2,
      });

      await idb.applySyncResponse({ cards: [fromServer], deletions: [] }, []);

      const stored = await idb.getCard(BOOK, "recognition");
      expect(stored?.stability).toBe(4.2);
    });

    it("removes locally-stored cards listed in server deletions", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));

      await idb.applySyncResponse(
        { cards: [], deletions: [cardId(CAT, "recognition")] },
        [],
      );

      expect(await idb.getCard(CAT, "recognition")).toBeNull();
    });

    it("does not re-tombstone server-reported deletions (tombstone set converges)", async () => {
      // If the server tells us "X is deleted", we shouldn't write a new
      // local tombstone — we'd just send it back next sync forever.
      await idb.applySyncResponse(
        { cards: [], deletions: [cardId(CAT, "recognition")] },
        [],
      );

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("clears tombstones we successfully forwarded", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      await idb.deleteCard(CAT);
      const sent = await idb.getAllTombstones();
      expect(sent.length).toBeGreaterThan(0);

      // Server acks: deletions list reflects everything it knows, including
      // the ids we just sent.
      await idb.applySyncResponse({ cards: [], deletions: sent }, sent);

      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("is a no-op when the server returns no changes", async () => {
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));

      await idb.applySyncResponse({ cards: [], deletions: [] }, []);

      expect(await idb.getCard(CAT, "recognition")).not.toBeNull();
    });

    it("tolerates a missing 'deletions' field for backwards-compat", async () => {
      // Older servers may not send the field at all.
      const card = makeCard({ sequence: BOOK });
      await idb.applySyncResponse({ cards: [card] }, []);
      expect(await idb.getCard(BOOK, "recognition")).not.toBeNull();
    });

    it("survives re-add during sync (Bug 1 regression)", async () => {
      // Scenario:
      //   1. user deletes 猫 → tombstone written, card removed
      //   2. auto-sync fires, reads sentTombstones = [id]
      //   3. while POST is in flight, user re-adds 猫 → card written back
      //   4. server response arrives with deletions = [id]
      // We must NOT delete the re-added card. The id was in sentTombstones,
      // so apply_remote_deletions should skip it.
      const id = cardId(CAT, "recognition");
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      // Simulate the re-add: tombstone gone (cleared after delete-and-readd
      // would have happened in real flow), card present.
      const sentTombstones = [id];

      await idb.applySyncResponse(
        { cards: [], deletions: [id] },
        sentTombstones,
      );

      // Card must survive.
      expect(await idb.getCard(CAT, "recognition")).not.toBeNull();
    });

    it("still applies deletions that another device originated", async () => {
      // Same shape as the race test, but this time we did NOT send a
      // tombstone for the id → the server is telling us about a foreign
      // delete, which we should apply.
      const id = cardId(CAT, "recognition");
      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));

      await idb.applySyncResponse({ cards: [], deletions: [id] }, []);

      expect(await idb.getCard(CAT, "recognition")).toBeNull();
    });

    it("does not clobber a card with a newer local last_review_ms", async () => {
      // Scenario: sync goes out, user reviews 猫 during the round-trip
      // (newer last_review_ms locally), server returns the old version.
      // Server's older copy must NOT overwrite the freshly-reviewed local
      // card.
      await idb.putCard(
        makeCard({
          sequence: CAT,
          direction: "recognition",
          stability: 9.0,
          last_review_ms: 2_000,
        }),
      );
      const olderFromServer = makeCard({
        sequence: CAT,
        direction: "recognition",
        stability: 1.0,
        last_review_ms: 1_000,
      });

      await idb.applySyncResponse({ cards: [olderFromServer], deletions: [] }, []);

      const local = await idb.getCard(CAT, "recognition");
      expect(local?.stability).toBe(9.0);
    });

    it("accepts a server card when its last_review_ms is newer", async () => {
      await idb.putCard(
        makeCard({
          sequence: CAT,
          direction: "recognition",
          stability: 1.0,
          last_review_ms: 1_000,
        }),
      );
      const newerFromServer = makeCard({
        sequence: CAT,
        direction: "recognition",
        stability: 9.0,
        last_review_ms: 2_000,
      });

      await idb.applySyncResponse({ cards: [newerFromServer], deletions: [] }, []);

      const local = await idb.getCard(CAT, "recognition");
      expect(local?.stability).toBe(9.0);
    });
  });

  describe("v5 → v7 clean reset", () => {
    // Cards used to key on a surface `word` string; v6/v7 re-key them on JMdict
    // `sequence`. There is no migration: the old word-keyed store is dropped
    // and recreated empty (users re-import). Verify nothing survives the bump.
    function openV5(): Promise<IDBDatabase> {
      return new Promise((resolve, reject) => {
        const req = indexedDB.open("yomeru-db", 5);
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
          db.createObjectStore("tombstones", { keyPath: "id" });
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      });
    }

    async function seedV5(db: IDBDatabase, cards: Array<Record<string, unknown>>) {
      await new Promise<void>((resolve, reject) => {
        const t = db.transaction(["cards", "tombstones"], "readwrite");
        const store = t.objectStore("cards");
        for (const c of cards) store.add(c);
        t.objectStore("tombstones").add({ id: "猫::recognition", deleted_at: 1 });
        t.oncomplete = () => resolve();
        t.onerror = () => reject(t.error);
      });
    }

    it("drops legacy word-keyed cards and tombstones, leaving empty stores", async () => {
      const v5 = await openV5();
      await seedV5(v5, [
        {
          id: "猫::recognition",
          word: "猫",
          direction: "recognition",
          due_ms: 100,
          stability: 4,
          difficulty: 5,
          reps: 3,
          lapses: 0,
          state: "review",
          last_review_ms: null,
          added_ms: 50,
          status: "active",
        },
      ]);
      v5.close();

      // Opening (any read goes through openDb at v6) triggers the reset.
      expect(await idb.getAllCards()).toEqual([]);
      expect(await idb.getAllTombstones()).toEqual([]);
    });

    it("the recreated store accepts sequence-keyed cards", async () => {
      const v5 = await openV5();
      await seedV5(v5, []);
      v5.close();

      await idb.putCard(makeCard({ sequence: CAT, direction: "recognition" }));
      const siblings = await idb.getCardsBySequence(CAT);
      expect(siblings).toHaveLength(1);
    });

    it("leaves a fresh install (oldVersion = 0) with an empty cards store", async () => {
      expect(await idb.getAllCards()).toEqual([]);
    });
  });
});
