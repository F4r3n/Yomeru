/**
 * Dev-only auto sign-in for `cargo xtask dev`.
 *
 * xtask writes `dev-config.json` into the extension directory while it runs a
 * dev-mode yomeru-server, and deletes it when Firefox closes. On startup, a
 * development install (temporary add-on / web-ext) that finds the file and has
 * no server configured yet points itself at that server and signs in; dev mode
 * returns a session token straight from /api/auth/request, so no code is
 * needed.
 *
 * Never reaches a release: the file isn't in `xtask package`'s file list, and
 * signed installs report `installType: "normal"` and stop before fetching it.
 * An already configured `serverUrl` is never overwritten.
 */

import { getSettings, saveSettings } from "./settings";
import { allowSyncWithoutConsent } from "./consent";
import { handleRequestOtp, handleSyncCards } from "./sync";

interface DevConfig {
  serverUrl: string;
  email: string;
}

const DEV_CONFIG_FILE = "dev-config.json";

async function readDevConfig(): Promise<DevConfig | null> {
  try {
    const res = await fetch(browser.runtime.getURL(DEV_CONFIG_FILE));
    if (!res.ok) return null;
    const cfg = (await res.json()) as Partial<DevConfig>;
    if (typeof cfg.serverUrl !== "string" || !cfg.serverUrl) return null;
    if (typeof cfg.email !== "string" || !cfg.email) return null;
    return { serverUrl: cfg.serverUrl, email: cfg.email };
  } catch {
    // Missing file: every setup other than `cargo xtask dev`.
    return null;
  }
}

export async function applyDevConfig(): Promise<void> {
  try {
    const self = await browser.management.getSelf();
    if (self.installType !== "development") return;

    const cfg = await readDevConfig();
    if (!cfg) return;
    // No click to ask from, and the server is a throwaway local one. Set on
    // every startup, so it holds for an already-configured dev profile too.
    allowSyncWithoutConsent();

    const settings = await getSettings();
    if (settings.serverUrl) return;

    await saveSettings({ ...settings, serverUrl: cfg.serverUrl, serverEmail: cfg.email });
    const res = await handleRequestOtp(cfg);
    if ("error" in res) {
      console.warn("[yomeru] dev auto sign-in failed:", res.error);
      return;
    }
    if (!("token" in res)) {
      // 204: the server sent an OTP email instead, so it isn't in dev mode.
      console.warn("[yomeru] dev auto sign-in: server is not in dev mode");
      return;
    }
    console.info(`[yomeru] dev: signed in to ${cfg.serverUrl} as ${cfg.email}`);
    await handleSyncCards();
  } catch (e) {
    console.warn("[yomeru] dev config skipped:", e);
  }
}
