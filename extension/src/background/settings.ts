import { DEFAULT_SETTINGS, type SrsSettings } from "../shared/types.ts";

/** The scheduler fields that sync between devices; the rest are device-local. */
const SYNCED_KEYS = [
  "graduationIntervalDays",
  "intervalScale",
  "maxSessionCards",
  "requestRetention",
] as const;

/** Synced scheduler settings as they cross `/api/sync` (snake_case). */
export interface SettingsPayload {
  graduation_interval_days: number;
  interval_scale: number;
  max_session_cards: number;
  request_retention: number;
  updated_ms: number;
}

export async function getSettings(): Promise<SrsSettings> {
  const res = await browser.storage.local.get("srs_settings");
  return { ...DEFAULT_SETTINGS, ...(res.srs_settings ?? {}) };
}

/**
 * Saves `s`, advancing the sync merge key only when a scheduler field actually
 * changed. Saving the server URL, email or token is a device-local concern and
 * must not let a stale scheduler config win a later sync. The stored key is
 * kept otherwise, since callers may hand back a copy read before a sync
 * adopted newer settings.
 */
export async function saveSettings(s: SrsSettings): Promise<void> {
  const prev = await getSettings();
  const changed = SYNCED_KEYS.some((k) => prev[k] !== s[k]);
  await browser.storage.local.set({
    srs_settings: {
      ...s,
      settingsUpdatedMs: changed ? Date.now() : prev.settingsUpdatedMs,
    },
  });
}

/** The payload to upload, or `undefined` if this device never edited them. */
export function toSettingsPayload(s: SrsSettings): SettingsPayload | undefined {
  // Untouched defaults carry no information; sending them would only create a
  // server row for other devices to compare against.
  if (!(s.settingsUpdatedMs > 0)) return undefined;
  return {
    graduation_interval_days: s.graduationIntervalDays,
    interval_scale: s.intervalScale,
    max_session_cards: s.maxSessionCards,
    request_retention: s.requestRetention,
    updated_ms: s.settingsUpdatedMs,
  };
}

/**
 * Adopts the server's settings if they're newer than ours, keeping their
 * timestamp rather than stamping a fresh one (that would bounce them back as a
 * local edit). Re-reads storage so an edit made while the sync was in flight
 * isn't clobbered.
 */
export async function adoptRemoteSettings(remote: SettingsPayload): Promise<void> {
  const cur = await getSettings();
  if (!(remote.updated_ms > cur.settingsUpdatedMs)) return;
  await browser.storage.local.set({
    srs_settings: {
      ...cur,
      graduationIntervalDays: remote.graduation_interval_days,
      intervalScale: remote.interval_scale,
      maxSessionCards: remote.max_session_cards,
      requestRetention: remote.request_retention,
      settingsUpdatedMs: remote.updated_ms,
    },
  });
}
