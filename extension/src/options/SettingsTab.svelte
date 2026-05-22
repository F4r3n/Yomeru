<script lang="ts">
    import type { SrsCard, SrsSettings } from "../shared/types.ts";
    import { DEFAULT_SETTINGS } from "../shared/types.ts";

    let settings = $state<SrsSettings>({ ...DEFAULT_SETTINGS });
    let saved = $state(false);
    let backupStatus = $state("");
    let backupError = $state(false);
    let dragging = $state(false);
    let otpSent = $state(false);
    let otpCode = $state("");
    let syncStatus = $state("");
    let syncError = $state(false);
    let syncBusy = $state(false);

    $effect(() => {
        browser.runtime.sendMessage({ type: "GET_SETTINGS" }).then((res) => {
            settings = (res as SrsSettings) ?? { ...DEFAULT_SETTINGS };
        });
    });

    async function save() {
        const payload: SrsSettings = {
            graduationReps: Number(settings.graduationReps),
            intervalScale: Number(settings.intervalScale),
            maxSessionCards: Number(settings.maxSessionCards),
            serverUrl: settings.serverUrl.trim(),
            serverEmail: settings.serverEmail.trim(),
            serverToken: settings.serverToken,
        };
        await browser.runtime.sendMessage({ type: "SAVE_SETTINGS", payload });
        saved = true;
        setTimeout(() => { saved = false; }, 2000);
    }

    function flashSync(msg: string, error = false) {
        syncStatus = msg;
        syncError = error;
        setTimeout(() => { syncStatus = ""; syncError = false; }, 6000);
    }

    async function requestOtp() {
        syncBusy = true;
        const res = await browser.runtime.sendMessage({
            type: "REQUEST_OTP",
            payload: { serverUrl: settings.serverUrl.trim(), email: settings.serverEmail.trim() },
        });
        syncBusy = false;
        const r = res as { success?: boolean; error?: string };
        if (r.error) { flashSync(r.error, true); return; }
        otpSent = true;
    }

    async function verifyOtp() {
        syncBusy = true;
        const res = await browser.runtime.sendMessage({
            type: "VERIFY_OTP",
            payload: {
                serverUrl: settings.serverUrl.trim(),
                email: settings.serverEmail.trim(),
                code: otpCode.trim(),
            },
        });
        syncBusy = false;
        const r = res as { success?: boolean; error?: string };
        if (r.error) { flashSync(r.error, true); return; }
        otpSent = false;
        otpCode = "";
        flashSync("Authenticated. You can now sync.");
    }

    async function syncNow() {
        syncBusy = true;
        const res = await browser.runtime.sendMessage({ type: "SYNC_CARDS" });
        syncBusy = false;
        const r = res as { synced?: number; queued?: boolean; error?: string };
        if (r.error) { flashSync(r.error, true); return; }
        if (r.queued) {
            flashSync("Sync already in progress — will repeat when it finishes.");
            return;
        }
        flashSync(`Synced ${r.synced} card${r.synced !== 1 ? "s" : ""}.`);
    }

    function flashBackup(msg: string, error = false) {
        backupStatus = msg;
        backupError = error;
        setTimeout(() => { backupStatus = ""; backupError = false; }, 6000);
    }

    async function exportCards() {
        try {
            const res = await browser.runtime.sendMessage({ type: "GET_ALL_CARDS" });
            const cards = (res as { cards: SrsCard[] }).cards ?? [];
            const payload = {
                version: browser.runtime.getManifest().version,
                exportedAt: Date.now(),
                cards,
            };
            const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = `yomeru-cards-${new Date().toISOString().slice(0, 10)}.json`;
            a.click();
            URL.revokeObjectURL(url);
            flashBackup(`Exported ${cards.length} card${cards.length !== 1 ? "s" : ""}.`);
        } catch (e) {
            flashBackup(`Export failed: ${e instanceof Error ? e.message : String(e)}`, true);
        }
    }

    async function importFromFile(file: File) {
        try {
            const data = JSON.parse(await file.text());
            const cards = data?.cards;
            if (!Array.isArray(cards)) throw new Error("file is missing a 'cards' array");
            const res = await browser.runtime.sendMessage({
                type: "IMPORT_CARDS",
                payload: { cards },
            });
            const r = res as { added: number; skipped: number; error?: string };
            if (r.error) throw new Error(r.error);
            flashBackup(`Imported ${r.added} card${r.added !== 1 ? "s" : ""}, skipped ${r.skipped} existing.`);
        } catch (err) {
            flashBackup(`Import failed: ${err instanceof Error ? err.message : String(err)}`, true);
        }
    }

    function onDragOver(e: DragEvent) {
        e.preventDefault();
        dragging = true;
    }
    function onDragLeave() {
        dragging = false;
    }
    function onDrop(e: DragEvent) {
        e.preventDefault();
        dragging = false;
        const file = e.dataTransfer?.files?.[0];
        if (file) importFromFile(file);
    }
</script>

<div class="settings-form">
    <div class="settings-row">
        <label class="settings-label" for="graduationReps">Graduate after N successes</label>
        <div class="settings-control">
            <input id="graduationReps" type="number" min="0" bind:value={settings.graduationReps} />
            <span class="settings-hint">0 = never graduate</span>
        </div>
    </div>
    <div class="settings-row">
        <label class="settings-label" for="intervalScale">Interval scale</label>
        <div class="settings-control">
            <input id="intervalScale" type="range" min="0.25" max="3" step="0.25" bind:value={settings.intervalScale} />
            <span class="settings-scale-value">×{settings.intervalScale.toFixed(2)}</span>
        </div>
    </div>
    <div class="settings-row">
        <label class="settings-label" for="maxSessionCards">Max cards per session</label>
        <div class="settings-control">
            <input id="maxSessionCards" type="number" min="1" max="200" bind:value={settings.maxSessionCards} />
        </div>
    </div>
    <div class="settings-actions">
        <button class="btn-save" onclick={save}>Save</button>
        {#if saved}<span class="settings-saved">Saved!</span>{/if}
    </div>

    <div class="settings-divider"></div>

    <div class="settings-row">
        <span class="settings-label">Backup &amp; Restore</span>
        <span class="settings-hint">Export your cards as JSON, then drop the file back here to restore. Existing cards are kept on import.</span>
        <div class="backup-actions">
            <button class="btn-backup" onclick={exportCards}>Export JSON</button>
        </div>
        <div
            class="drop-zone"
            class:dragging
            ondragover={onDragOver}
            ondragleave={onDragLeave}
            ondrop={onDrop}
            role="region"
            aria-label="Drop JSON file here to import"
        >
            <span class="drop-zone-icon">⬇</span>
            <span class="drop-zone-label">Drop JSON file here to import</span>
        </div>
        {#if backupStatus}
            <span class="backup-status" class:backup-status--error={backupError}>{backupStatus}</span>
        {/if}
    </div>

    <div class="settings-divider"></div>

    <div class="settings-row">
        <span class="settings-label">Sync Server</span>
        <span class="settings-hint">Enter your server URL and email. A one-time code will be emailed to you.</span>
        <div class="settings-control sync-inputs">
            <input type="url" placeholder="http://localhost:8080" bind:value={settings.serverUrl} />
            <input type="email" placeholder="your@email.com" bind:value={settings.serverEmail} />
        </div>
        {#if !otpSent}
            <div class="backup-actions">
                <button class="btn-backup" onclick={requestOtp} disabled={syncBusy}>
                    {syncBusy ? "Sending…" : "Send code"}
                </button>
                <button class="btn-backup" onclick={syncNow}
                    disabled={syncBusy || !settings.serverToken}>
                    {syncBusy ? "Syncing…" : "Sync now"}
                </button>
            </div>
        {:else}
            <span class="settings-hint" style="margin-top:6px;">Check your email for a 6-digit code:</span>
            <div class="settings-control" style="margin-top:4px;">
                <input class="otp-input" type="text" inputmode="numeric" maxlength="6"
                    placeholder="000000" bind:value={otpCode} />
                <button class="btn-backup" onclick={verifyOtp} disabled={syncBusy}>
                    {syncBusy ? "Verifying…" : "Verify"}
                </button>
            </div>
        {/if}
        {#if syncStatus}
            <span class="backup-status" class:backup-status--error={syncError}>{syncStatus}</span>
        {/if}
    </div>
</div>

<style>
    .settings-form {
        display: flex;
        flex-direction: column;
        gap: 16px;
        max-width: 400px;
    }
    .settings-row {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .settings-label {
        font-size: 13px;
        color: var(--subtext);
    }
    .settings-control {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .settings-control input[type="number"] {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        font-size: 13px;
        padding: 4px 8px;
        width: 80px;
        outline: none;
    }
    .settings-control input[type="number"]:focus {
        border-color: var(--accent);
    }
    .settings-control input[type="range"] {
        accent-color: var(--accent);
        width: 140px;
    }
    .settings-hint {
        font-size: 11px;
        color: var(--subtext);
    }
    .settings-scale-value {
        font-size: 13px;
        color: var(--accent);
        min-width: 36px;
    }
    .settings-actions {
        display: flex;
        align-items: center;
        gap: 12px;
        margin-top: 4px;
    }
    .btn-save {
        background: var(--accent);
        border: none;
        border-radius: 6px;
        color: var(--bg);
        cursor: pointer;
        font-size: 13px;
        font-weight: 600;
        padding: 6px 24px;
        transition: opacity 0.15s;
    }
    .btn-save:hover {
        opacity: 0.85;
    }
    .settings-saved {
        font-size: 12px;
        color: var(--green);
    }
    .settings-divider {
        height: 1px;
        background: var(--border);
        margin: 8px 0;
    }
    .backup-actions {
        display: flex;
        gap: 8px;
        margin-top: 6px;
    }
    .btn-backup {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        cursor: pointer;
        font-size: 13px;
        padding: 6px 14px;
        transition: border-color 0.15s, background 0.15s;
        display: inline-block;
    }
    .btn-backup:hover {
        border-color: var(--accent);
    }
    .drop-zone {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 6px;
        margin-top: 8px;
        padding: 18px 12px;
        border: 2px dashed var(--border);
        border-radius: 8px;
        color: var(--subtext);
        text-align: center;
        transition: border-color 0.15s, background 0.15s, color 0.15s;
    }
    .drop-zone.dragging {
        border-color: var(--accent);
        background: rgba(203, 166, 247, 0.08);
        color: var(--accent);
    }
    .drop-zone-icon {
        font-size: 18px;
        line-height: 1;
    }
    .drop-zone-label {
        font-size: 12px;
    }
    .backup-status {
        font-size: 12px;
        color: var(--green);
        margin-top: 6px;
    }
    .backup-status--error {
        color: var(--red);
    }
    .sync-inputs {
        flex-direction: column;
        align-items: flex-start;
        gap: 6px;
        margin-top: 6px;
    }
    .sync-inputs input,
    .settings-control input[type="url"],
    .settings-control input[type="email"] {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        font-size: 13px;
        padding: 4px 8px;
        width: 240px;
        outline: none;
    }
    .sync-inputs input:focus,
    .settings-control input[type="url"]:focus,
    .settings-control input[type="email"]:focus {
        border-color: var(--accent);
    }
    .otp-input {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        font-size: 16px;
        letter-spacing: 6px;
        outline: none;
        padding: 4px 8px;
        width: 100px;
    }
    .otp-input:focus {
        border-color: var(--accent);
    }
</style>
