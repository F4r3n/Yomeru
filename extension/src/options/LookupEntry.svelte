<script lang="ts">
    import type { WordEntry, KanjiEntry, ExampleEntry } from "../shared/types.ts";
    import EntryCard from "../content/EntryCard.svelte";
    import KanjiTab from "../content/KanjiTab.svelte";
    import ExamplesTab from "../content/ExamplesTab.svelte";

    let {
        entry,
        btnState,
        onadd,
    }: {
        entry: WordEntry;
        btnState: "idle" | "added" | "existing";
        onadd: (word: string) => void;
    } = $props();

    let headword = $derived(entry.kanji_forms[0]?.text ?? entry.reading_forms[0]?.text ?? "");
    let expanded = $state(false);
    let loading = $state(false);
    let loaded = $state(false);
    let kanjiEntries = $state<KanjiEntry[]>([]);
    let examples = $state<ExampleEntry[]>([]);

    async function toggle() {
        expanded = !expanded;
        if (expanded && !loaded && !loading) {
            loading = true;
            try {
                const [kanjiRes, exRes] = await Promise.all([
                    browser.runtime.sendMessage({ type: "GET_KANJI", payload: { word: headword } }),
                    browser.runtime.sendMessage({ type: "GET_EXAMPLES", payload: { word: headword } }),
                ]);
                kanjiEntries = (kanjiRes as { entries: KanjiEntry[] }).entries ?? [];
                examples = (exRes as { entries: ExampleEntry[] }).entries ?? [];
            } finally {
                loading = false;
                loaded = true;
            }
        }
    }
</script>

<div class="lookup-entry">
    <EntryCard {entry} {btnState} {onadd} />
    <button class="entry-more" onclick={toggle} aria-expanded={expanded}>
        {expanded ? "▾" : "▸"} Kanji & examples
    </button>
    {#if expanded}
        <div class="entry-detail">
            {#if loading}
                <div class="entry-detail-empty">Loading…</div>
            {:else}
                {#if kanjiEntries.length > 0}
                    <KanjiTab {kanjiEntries} />
                {/if}
                <ExamplesTab {examples} fetched={loaded} word={headword} />
                {#if kanjiEntries.length === 0 && examples.length === 0 && loaded}
                    <div class="entry-detail-empty">No extra detail available.</div>
                {/if}
            {/if}
        </div>
    {/if}
</div>

<style>
    .lookup-entry {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .entry-more {
        align-self: flex-start;
        background: none;
        border: none;
        color: var(--subtext);
        cursor: pointer;
        font-size: 12px;
        padding: 2px 0;
    }
    .entry-more:hover {
        color: var(--text);
    }
    .entry-detail {
        border-left: 2px solid var(--border);
        padding: 4px 0 4px 10px;
    }
    .entry-detail-empty {
        color: var(--subtext);
        font-size: 12px;
        padding: 4px 0;
    }
</style>
