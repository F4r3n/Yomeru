<script lang="ts">
    import type { WordEntry } from "../shared/types.ts";
    import LookupEntry from "./LookupEntry.svelte";
    import { POPUP_CSS } from "../content/popup.css.ts";
    import { lookupAllEntries } from "./dict-lookup.ts";
    import { watchCardsDb } from "./db-watch.ts";
    import { isRomaji, romajiToHiragana } from "./romaji.ts";
    import { loadHistory, pushHistory, clearHistory } from "./lookup-history.ts";

    let query = $state("");
    let lastTarget = $state("");
    let entries = $state<WordEntry[]>([]);
    let searching = $state(false);
    let searched = $state(false);
    let buttonStates = $state<Record<string, "idle" | "added" | "existing">>({});
    let history = $state<string[]>([]);

    let pending: ReturnType<typeof setTimeout> | null = null;

    loadHistory().then((h) => { history = h; });

    async function loadSrsWords() {
        const res = await browser.runtime.sendMessage({ type: "GET_SRS_WORDS" }) as { words?: string[] };
        const next: Record<string, "idle" | "existing"> = {};
        for (const w of res.words ?? []) next[w] = "existing";
        buttonStates = next;
    }

    $effect(() => watchCardsDb(loadSrsWords));

    function onInput() {
        if (pending) clearTimeout(pending);
        pending = setTimeout(runLookup, 200);
    }

    async function runLookup() {
        pending = null;
        const q = query.trim();
        if (!q) {
            entries = [];
            searched = false;
            return;
        }
        const target = isRomaji(q) ? romajiToHiragana(q) : q;
        lastTarget = target;
        searching = true;
        try {
            entries = await lookupAllEntries(target);
        } finally {
            searching = false;
            searched = true;
        }
    }

    async function commitToHistory() {
        if (pending) { clearTimeout(pending); pending = null; }
        await runLookup();
        if (entries.length > 0 && lastTarget) {
            history = await pushHistory(lastTarget);
        }
    }

    function onKeyDown(e: KeyboardEvent) {
        if (e.key === "Enter") {
            e.preventDefault();
            commitToHistory();
        }
    }

    function searchFor(word: string) {
        if (pending) { clearTimeout(pending); pending = null; }
        query = word;
        runLookup();
    }

    async function onClearHistory() {
        await clearHistory();
        history = [];
    }

    async function addToSrs(word: string) {
        const res = await browser.runtime.sendMessage({
            type: "ADD_WORD",
            payload: { word },
        }) as { existing?: boolean };
        buttonStates = {
            ...buttonStates,
            [word]: res.existing ? "existing" : "added",
        };
    }
</script>

<svelte:head>
    {@html `<style>${POPUP_CSS}</style>`}
</svelte:head>

<div class="lookup-wrap">
    <div class="lookup-row">
        <input
            class="lookup-input"
            type="search"
            placeholder="Type a Japanese word…"
            bind:value={query}
            oninput={onInput}
            onkeydown={onKeyDown}
            autofocus
        />
        <button class="lookup-search-btn" onclick={commitToHistory} disabled={!query.trim()}>
            Search
        </button>
    </div>

    {#if isRomaji(query.trim()) && lastTarget && lastTarget !== query.trim()}
        <div class="lookup-converted">→ {lastTarget}</div>
    {/if}

    {#if history.length > 0 && entries.length === 0 && !searching}
        <div class="lookup-history">
            <div class="lookup-history-header">
                <span class="lookup-history-label">Recent</span>
                <button class="lookup-history-clear" onclick={onClearHistory} title="Clear history">Clear</button>
            </div>
            <ul class="lookup-history-list">
                {#each history as h}
                    <li>
                        <button class="lookup-history-item" onclick={() => searchFor(h)}>{h}</button>
                    </li>
                {/each}
            </ul>
        </div>
    {/if}

    {#if searching}
        <div class="lookup-empty">Searching…</div>
    {:else if entries.length > 0}
        <div class="lookup-results">
            {#each entries as entry, i (entry.sequence)}
                {#if i > 0}<hr class="lookup-divider" />{/if}
                {@const hw = entry.kanji_forms[0]?.text ?? entry.reading_forms[0]?.text ?? ""}
                <LookupEntry
                    {entry}
                    btnState={buttonStates[hw] ?? "idle"}
                    onadd={addToSrs}
                />
            {/each}
        </div>
    {:else if searched}
        <div class="lookup-empty">No entry found for 「{lastTarget || query}」.</div>
    {/if}
</div>

<style>
    .lookup-wrap {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .lookup-row {
        display: flex;
        gap: 8px;
    }
    .lookup-input {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        flex: 1;
        font-size: 16px;
        padding: 8px 12px;
        outline: none;
    }
    .lookup-input:focus {
        border-color: var(--accent);
    }
    .lookup-search-btn {
        background: var(--accent);
        border: none;
        border-radius: 6px;
        color: var(--bg);
        cursor: pointer;
        font-size: 14px;
        font-weight: 600;
        padding: 0 16px;
    }
    .lookup-search-btn:disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }
    .lookup-empty {
        color: var(--subtext);
        font-size: 13px;
        padding: 8px 0;
        text-align: center;
    }
    .lookup-converted {
        color: var(--green);
        font-size: 14px;
        margin-top: -4px;
    }
    .lookup-history {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .lookup-history-header {
        align-items: center;
        display: flex;
        justify-content: space-between;
    }
    .lookup-history-label {
        color: var(--subtext);
        font-size: 12px;
        text-transform: uppercase;
        letter-spacing: 0.05em;
    }
    .lookup-history-clear {
        background: none;
        border: none;
        color: var(--subtext);
        cursor: pointer;
        font-size: 12px;
        padding: 2px 4px;
    }
    .lookup-history-clear:hover {
        color: var(--text);
    }
    .lookup-history-list {
        display: flex;
        flex-direction: column;
        list-style: none;
        margin: 0;
        padding: 0;
    }
    .lookup-history-item {
        background: none;
        border: none;
        border-bottom: 1px solid var(--border);
        color: var(--text);
        cursor: pointer;
        font-size: 14px;
        padding: 6px 8px;
        text-align: left;
        width: 100%;
    }
    .lookup-history-item:hover {
        background: var(--surface);
    }
    .lookup-results :global(.jp-popup),
    .lookup-results :global(.jp-entry) {
        background: transparent;
    }
    .lookup-divider {
        border: none;
        border-top: 1px solid var(--border);
        margin: 8px 0;
    }
</style>
