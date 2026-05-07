import { DEFAULT_SETTINGS, type SrsSettings } from "../shared/types.ts";

export async function getSettings(): Promise<SrsSettings> {
  const res = await browser.storage.local.get("srs_settings");
  return { ...DEFAULT_SETTINGS, ...(res.srs_settings ?? {}) };
}

export async function saveSettings(s: SrsSettings): Promise<void> {
  await browser.storage.local.set({ srs_settings: s });
}
