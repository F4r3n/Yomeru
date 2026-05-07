<script lang="ts">
    import type { SrsSettings } from "../shared/types.ts";
    import { DEFAULT_SETTINGS } from "../shared/types.ts";

    let settings = $state<SrsSettings>({ ...DEFAULT_SETTINGS });
    let saved = $state(false);

    $effect(() => {
        browser.runtime.sendMessage({ type: "GET_SETTINGS" }).then((res) => {
            settings = (res as SrsSettings) ?? { ...DEFAULT_SETTINGS };
        });
    });

    async function save() {
        await browser.runtime.sendMessage({ type: "SAVE_SETTINGS", payload: settings });
        saved = true;
        setTimeout(() => { saved = false; }, 2000);
    }
</script>

<div class="settings-form">
    <div class="settings-row">
        <label class="settings-label" for="maxStagingSize">Max staging list size</label>
        <div class="settings-control">
            <input id="maxStagingSize" type="number" min="0" bind:value={settings.maxStagingSize} />
            <span class="settings-hint">0 = unlimited</span>
        </div>
    </div>
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
</style>
