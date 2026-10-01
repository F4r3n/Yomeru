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
      expect(s.graduationIntervalDays).toBe(DEFAULT_SETTINGS.graduationIntervalDays);
      expect(s.intervalScale).toBe(DEFAULT_SETTINGS.intervalScale);
    });

    it("falls back to defaults for keys not present in stored object", async () => {
      storage.set("srs_settings", { maxSessionCards: 10 });

      const s = await settings.getSettings();

      expect(s.maxSessionCards).toBe(10);
      expect(s.graduationIntervalDays).toBe(DEFAULT_SETTINGS.graduationIntervalDays);
    });
  });

  describe("saveSettings", () => {
    it("persists settings that getSettings reads back", async () => {
      const custom: SrsSettings = {
        graduationIntervalDays: 5,
        intervalScale: 1.5,
        maxSessionCards: 15,
        requestRetention: 0.85,
        settingsUpdatedMs: 0,
        serverUrl: "",
        serverEmail: "",
        serverToken: "",
      };

      await settings.saveSettings(custom);

      const { settingsUpdatedMs, ...rest } = await settings.getSettings();
      const { settingsUpdatedMs: _, ...expected } = custom;
      expect(rest).toEqual(expected);
      expect(settingsUpdatedMs).toBeGreaterThan(0);
    });

    it("overwrites previously stored settings", async () => {
      await settings.saveSettings({ ...DEFAULT_SETTINGS, maxSessionCards: 5 });
      await settings.saveSettings({ ...DEFAULT_SETTINGS, maxSessionCards: 99 });

      expect((await settings.getSettings()).maxSessionCards).toBe(99);
    });

    it("stamps settingsUpdatedMs when a scheduler field changes", async () => {
      const clock = vi.spyOn(Date, "now").mockReturnValue(5_000);

      await settings.saveSettings({ ...DEFAULT_SETTINGS, requestRetention: 0.8 });
      clock.mockRestore();

      expect((await settings.getSettings()).settingsUpdatedMs).toBe(5_000);
    });

    it("keeps settingsUpdatedMs when only device-local fields change", async () => {
      // Saving a token must not make a stale scheduler config win a sync.
      storage.set("srs_settings", { ...DEFAULT_SETTINGS, settingsUpdatedMs: 1_000 });

      await settings.saveSettings({
        ...DEFAULT_SETTINGS,
        serverToken: "t",
        settingsUpdatedMs: 0,
      });

      expect((await settings.getSettings()).settingsUpdatedMs).toBe(1_000);
    });
  });

  describe("adoptRemoteSettings", () => {
    const remote = {
      graduation_interval_days: 30,
      interval_scale: 1.2,
      max_session_cards: 7,
      request_retention: 0.8,
      updated_ms: 2_000,
    };

    it("adopts newer settings, keeping their timestamp and local fields", async () => {
      storage.set("srs_settings", {
        ...DEFAULT_SETTINGS,
        serverToken: "t",
        settingsUpdatedMs: 1_000,
      });

      await settings.adoptRemoteSettings(remote);

      const s = await settings.getSettings();
      expect(s.requestRetention).toBe(0.8);
      expect(s.maxSessionCards).toBe(7);
      expect(s.settingsUpdatedMs).toBe(2_000);
      expect(s.serverToken).toBe("t");
    });

    it("ignores settings that are not newer than ours", async () => {
      storage.set("srs_settings", { ...DEFAULT_SETTINGS, settingsUpdatedMs: 3_000 });

      await settings.adoptRemoteSettings(remote);

      expect((await settings.getSettings()).requestRetention).toBe(0.9);
    });
  });
});
