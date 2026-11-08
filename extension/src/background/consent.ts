/**
 * Firefox's built-in data consent for sync.
 *
 * Sync sends the user's email address and sign-in data to a server, so the
 * manifest declares them as *optional* data collection, and nothing may leave
 * the device until the user grants them. The options page asks (it has to, from
 * a click: see `request_sync_consent` in app/extension/src/bridge.rs); the
 * background only checks, before every sign-in request and every sync. Users can
 * withdraw it in about:addons, which pauses sync until they allow it again.
 */

/** Keep in step with `optional` in manifest.json and `SYNC_DATA` in bridge.rs. */
export const SYNC_DATA_COLLECTION = ["personallyIdentifyingInfo", "authenticationInfo"];

export const NO_CONSENT_ERROR =
  "Sync is paused: allow Yomeru to send your email and sign-in data (Settings → Sync).";

let devBypass = false;

/** `cargo xtask dev` signs in with no click to hang a prompt on. Called by the
 *  dev auto sign-in only, which never runs outside a development install. */
export function allowSyncWithoutConsent(): void {
  devBypass = true;
}

export async function hasSyncConsent(): Promise<boolean> {
  if (devBypass) return true;
  const all = (await browser.permissions.getAll()) as { data_collection?: string[] };
  // A Firefox without built-in data consent has nothing to grant.
  if (!all.data_collection) return true;
  return SYNC_DATA_COLLECTION.every((c) => all.data_collection!.includes(c));
}
