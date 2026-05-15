import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS } from "../shared/types.ts";
import type { SrsSettings } from "../shared/types.ts";

type SettingsModule = typeof import("./settings.ts");

describe("settings", () => {
  let settings: SettingsModule;
  let storage: Map<string, unknown>;

  beforeEach(async () => {
    vi.resetModules();
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
    settings = await import("./settings.ts");
  });

  describe("getSettings", () => {
    it("returns DEFAULT_SETTINGS when nothing is stored", async () => {
      expect(await settings.getSettings()).toEqual(DEFAULT_SETTINGS);
    });

    it("merges stored values over defaults", async () => {
      await settings.saveSettings({ ...DEFAULT_SETTINGS, maxSessionCards: 5 });

      const s = await settings.getSettings();

      expect(s.maxSessionCards).toBe(5);
      expect(s.graduationReps).toBe(DEFAULT_SETTINGS.graduationReps);
      expect(s.intervalScale).toBe(DEFAULT_SETTINGS.intervalScale);
    });

    it("falls back to defaults for keys not present in stored object", async () => {
      storage.set("srs_settings", { maxSessionCards: 10 });

      const s = await settings.getSettings();

      expect(s.maxSessionCards).toBe(10);
      expect(s.graduationReps).toBe(DEFAULT_SETTINGS.graduationReps);
    });
  });

  describe("saveSettings", () => {
    it("persists settings that getSettings reads back", async () => {
      const custom: SrsSettings = {
        graduationReps: 5,
        intervalScale: 1.5,
        maxSessionCards: 15,
      };

      await settings.saveSettings(custom);

      expect(await settings.getSettings()).toEqual(custom);
    });

    it("overwrites previously stored settings", async () => {
      await settings.saveSettings({ ...DEFAULT_SETTINGS, maxSessionCards: 5 });
      await settings.saveSettings({ ...DEFAULT_SETTINGS, maxSessionCards: 99 });

      expect((await settings.getSettings()).maxSessionCards).toBe(99);
    });
  });
});
