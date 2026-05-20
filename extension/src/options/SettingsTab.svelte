<script lang="ts">
    import type { SrsCard, SrsSettings } from "../shared/types.ts";
    import { DEFAULT_SETTINGS } from "../shared/types.ts";

    let settings = $state<SrsSettings>({ ...DEFAULT_SETTINGS });
    let saved = $state(false);
    let backupStatus = $state("");
    let backupError = $state(false);
    let dragging = $state(false);

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
        };
        await browser.runtime.sendMessage({ type: "SAVE_SETTINGS", payload });
        saved = true;
        setTimeout(() => { saved = false; }, 2000);
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
</style>
