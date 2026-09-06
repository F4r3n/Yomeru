/**
 * What the extension actually puts on the wire, and what it does with what
 * comes back.
 *
 * The request is pinned against `testdata/sync/request-extension-client.json`
 * — the same file the server's HTTP tests POST through its real router — so
 * neither side of the contract can move without the other noticing. The
 * response half drives `doSync` through a mocked `fetch` and asserts the
 * resulting fake-indexeddb state, which is what makes the mid-flight races
 * below testable at all: they are races between the reads and the response,
 * and nothing below `doSync` can see both.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IDBFactory, IDBKeyRange as FakeIDBKeyRange } from "fake-indexeddb";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { SrsCard } from "../shared/types.ts";
import { cardId } from "../shared/types.ts";
import { makeCard } from "./test-helpers.ts";

type IdbModule = typeof import("./idb.ts");
type SyncModule = typeof import("./sync.ts");

const SERVER = "https://sync.example.invalid";
const TOKEN = "test-token";

const WORD = 1_467_640;
const CAT = 1_001;
const DOG = 1_002;

function fixture(name: string): unknown {
  const path = fileURLToPath(new URL(`../../../testdata/sync/${name}`, import.meta.url));
  return JSON.parse(readFileSync(path, "utf8"));
}

/**
 * JS has one number type, so `JSON.stringify` writes `1757000000123` where
 * Rust writes `1757000000123.0`. Comparing parsed values rather than text is
 * what lets the same fixture describe both dialects; the key-set assertion
 * alongside it covers the shape that this comparison can't see.
 */
function keysOf(o: unknown): string[] {
  return Object.keys(o as Record<string, unknown>).sort();
}

describe("sync", () => {
  let idb: IdbModule;
  let sync: SyncModule;
  let storage: Map<string, unknown>;
  let fetchMock: ReturnType<typeof vi.fn>;

  /** The bodies `doSync` posted, parsed, most recent last. */
  let sent: Array<Record<string, unknown>>;

  /**
   * Queue one response for the next `/api/sync` POST. `duringFlight` runs after
   * the request body has been captured and before the response is handed back —
   * i.e. exactly the window a user's review or card add can land in, which is
   * the whole subject of the race tests below.
   */
  function respondWith(
    body: unknown,
    { status = 200, duringFlight }: { status?: number; duringFlight?: () => Promise<void> } = {},
  ): void {
    fetchMock.mockImplementationOnce(async (_url: string, init: RequestInit) => {
      sent.push(JSON.parse(init.body as string));
      if (duringFlight) await duringFlight();
      return {
        ok: status >= 200 && status < 300,
        status,
        json: async () => body,
        text: async () => JSON.stringify(body),
      };
    });
  }

  beforeEach(async () => {
    vi.resetModules();
    globalThis.indexedDB = new IDBFactory();
    (globalThis as unknown as Record<string, unknown>).IDBKeyRange = FakeIDBKeyRange;
    storage = new Map();
    storage.set("srs_settings", { serverUrl: SERVER, serverToken: TOKEN });
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
    sent = [];
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    // The IDB layer logs its upgrade/open path on every fresh factory; left
    // alone it outlives the test file and trips vitest's teardown.
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
    idb = await import("./idb.ts");
    sync = await import("./sync.ts");
    // Importing sync.ts kicks off the backup restore. Let it settle here so it
    // can't land in the middle of a test — or after the file has finished.
    await sync.storageReady;
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  describe("the request body", () => {
    it("matches the shared wire fixture", async () => {
      // Seeded through putCardsSynced so the card keeps the exact updated_ms
      // the fixture names; putCard would re-stamp it with the wall clock.
      await idb.putCardsSynced([
        {
          id: cardId(WORD, "recognition"),
          sequence: WORD,
          direction: "recognition",
          due_ms: 1_757_000_000_000,
          stability: 4.2,
          difficulty: 5.5,
          reps: 3,
          lapses: 1,
          state: "review",
          last_review_ms: 1_756_900_000_000,
          added_ms: 1_756_000_000_000,
          status: "active",
          priority: 0,
          updated_ms: 1_756_900_000_000,
        },
      ]);
      // The tombstone's deleted_at is the wall clock at delete time, so pin it.
      // Stubbing `Date.now` rather than installing fake timers: fake-indexeddb
      // drives its transactions off the real event loop and never settles
      // under them.
      const clock = vi.spyOn(Date, "now").mockReturnValue(1_757_000_000_123);
      await idb.deleteCardById(cardId(WORD, "recall"));
      clock.mockRestore();

      respondWith({ cards: [], deletions: [] });
      await sync.handleSyncCards();

      const expected = fixture("request-extension-client.json") as Record<string, unknown>;
      expect(sent).toHaveLength(1);
      expect(sent[0]).toEqual(expected);
      // toEqual ignores nothing about keys, but say it outright: an extra field
      // (or a renamed one) is a wire change, and the server parses by name.
      expect(keysOf(sent[0])).toEqual(["cards", "deletions"]);
      expect(keysOf((sent[0].cards as unknown[])[0])).toEqual(
        keysOf((expected.cards as unknown[])[0]),
      );
      expect(keysOf((sent[0].deletions as unknown[])[0])).toEqual(["deleted_at", "id"]);
    });

    it("sends tombstones as {id, deleted_at} objects, not bare ids", async () => {
      // The server accepts both, but only the object form carries the time the
      // user actually deleted — a delete made offline last week must not be
      // stamped with today's upload.
      await idb.putCard(makeCard({ sequence: CAT }));
      await idb.deleteCard(CAT);

      respondWith({ cards: [], deletions: [] });
      await sync.handleSyncCards();

      const deletions = sent[0].deletions as Array<Record<string, unknown>>;
      expect(deletions).toHaveLength(2);
      for (const d of deletions) {
        expect(typeof d.id).toBe("string");
        expect(typeof d.deleted_at).toBe("number");
      }
    });

    it("leaves legacy word-keyed rows out of the upload", async () => {
      await idb.putCard(makeCard({ sequence: CAT }));
      await idb.putCardsSynced([
        { ...makeCard({ sequence: CAT }), id: "猫::recognition", sequence: NaN } as SrsCard,
      ]);

      respondWith({ cards: [], deletions: [] });
      await sync.handleSyncCards();

      const ids = (sent[0].cards as SrsCard[]).map((c) => c.id);
      expect(ids).toEqual([cardId(CAT, "recognition")]);
    });
  });

  describe("the response", () => {
    it("applies the shared response fixture", async () => {
      respondWith(fixture("response.json"));

      const result = await sync.handleSyncCards();

      expect(result).toEqual({ synced: 1 });
      const card = await idb.getCard(WORD, "recognition");
      expect(card).not.toBeNull();
      expect(card?.reps).toBe(3);
      // Adopted verbatim: re-stamping would mark it locally modified and bounce
      // it straight back at the server on the next round.
      expect(card?.updated_ms).toBe(1_756_900_000_000);
    });

    it("does not clobber a card reviewed while the sync was in flight", async () => {
      // The race wholesale replacement lost: the user reviews after the payload
      // goes out, so the copy the server echoes back is already stale.
      const uploaded = makeCard({ sequence: CAT, reps: 1, updated_ms: 1_000 });
      await idb.putCardsSynced([uploaded]);
      respondWith(
        { cards: [uploaded], deletions: [] },
        {
          duringFlight: async () => {
            await idb.putCard(makeCard({ sequence: CAT, reps: 5 }));
          },
        },
      );

      await sync.handleSyncCards();

      expect((await idb.getCard(CAT, "recognition"))?.reps).toBe(5);
    });

    it("does not delete a card added while the sync was in flight", async () => {
      // Added after the payload went out, so it was never in what the server
      // ruled on. Its absence from resp.cards means nothing, and the tombstone
      // the server still holds is about the *old* copy, not this one. Wholesale
      // replacement deleted it on both counts.
      //
      // `added_ms` is offset rather than left at `Date.now()`: the whole round
      // trip runs well inside one millisecond here, so a bare read ties with
      // the `uploadedAt` captured just before it, and the guard (`added <=
      // uploadedAt`) reads a tie as a card the server had already seen. That
      // boundary is its own question; this test is about the unambiguous case.
      respondWith(
        { cards: [], deletions: [cardId(DOG, "recognition")] },
        {
          duringFlight: async () => {
            await idb.putCard(makeCard({ sequence: DOG, added_ms: Date.now() + 1_000 }));
          },
        },
      );

      await sync.handleSyncCards();

      expect(await idb.getCard(DOG, "recognition")).not.toBeNull();
    });

    it("applies a server deletion for a card it has already ruled on", async () => {
      const id = cardId(CAT, "recognition");
      await idb.putCardsSynced([makeCard({ sequence: CAT, added_ms: 1_000 })]);
      respondWith({ cards: [], deletions: [id] });

      await sync.handleSyncCards();

      expect(await idb.getCard(CAT, "recognition")).toBeNull();
    });

    it("keeps a card whose tombstone we sent in this very request", async () => {
      // The server echoes back the tombstone we just gave it. Acting on that is
      // how a re-add gets silently eaten: we deleted, re-added, and the echo
      // would delete the new copy before the server ever saw it.
      await idb.putCard(makeCard({ sequence: CAT }));
      await idb.deleteCard(CAT);
      const readded = makeCard({ sequence: CAT, added_ms: Date.now() });
      await idb.putCard(readded);
      respondWith({ cards: [], deletions: [cardId(CAT, "recognition")] });

      await sync.handleSyncCards();

      expect(await idb.getCard(CAT, "recognition")).not.toBeNull();
    });

    it("clears the tombstones it forwarded", async () => {
      await idb.putCard(makeCard({ sequence: CAT }));
      await idb.deleteCard(CAT);
      expect(await idb.getAllTombstones()).toHaveLength(2);
      respondWith({ cards: [], deletions: [] });

      await sync.handleSyncCards();

      expect(await idb.getAllTombstones()).toHaveLength(0);
    });

    it("prunes legacy word-keyed rows the server can never return", async () => {
      // Never uploaded, so never in the response — merging alone would leave
      // them forever. They're local-only junk, so no tombstone is written.
      await idb.putCardsSynced([
        { ...makeCard({ sequence: CAT }), id: "猫::recognition", sequence: NaN } as SrsCard,
      ]);
      respondWith({ cards: [], deletions: [] });

      await sync.handleSyncCards();

      expect(await idb.getAllCards()).toHaveLength(0);
      expect(await idb.getAllTombstones()).toHaveLength(0);
    });

    it("parses a response with no deletions key", async () => {
      // What an older server returns. `deletions` is optional precisely so the
      // client can ship before the server does.
      await idb.putCard(makeCard({ sequence: CAT }));
      respondWith({ cards: [] });

      await sync.handleSyncCards();

      expect(await idb.getCard(CAT, "recognition")).not.toBeNull();
    });
  });

  describe("failures", () => {
    it("reports an expired session without touching local cards", async () => {
      await idb.putCard(makeCard({ sequence: CAT }));
      await idb.deleteCard(CAT);
      respondWith({}, { status: 401 });

      expect(await sync.handleSyncCards()).toEqual({
        error: "session expired — re-verify",
      });
      // The tombstones must survive: the server never received them.
      expect(await idb.getAllTombstones()).toHaveLength(2);
    });

    it("reports a server error without touching local cards", async () => {
      await idb.putCard(makeCard({ sequence: CAT, reps: 4 }));
      respondWith({}, { status: 500 });

      expect(await sync.handleSyncCards()).toEqual({ error: "server 500" });
      expect((await idb.getCard(CAT, "recognition"))?.reps).toBe(4);
    });

    it("refuses to sync when no server is configured", async () => {
      storage.set("srs_settings", { serverUrl: "", serverToken: "" });

      expect(await sync.handleSyncCards()).toEqual({ error: "not authenticated" });
      expect(fetchMock).not.toHaveBeenCalled();
    });
  });
});
