<script lang="ts">
    import type { SrsCard, WordEntry } from "../shared/types.ts";
    import { buildEntryMap, readingOf, meaningOf } from "./dict-lookup.ts";
    import { watchCardsDb } from "./db-watch.ts";

    let { onstagingchange }: { onstagingchange: (n: number) => void } = $props();

    let stagingCards = $state<SrsCard[]>([]);
    let entriesByWord = $state<Record<string, WordEntry | null>>({});

    // Dedupe by word: both direction siblings share status, so one row per word.
    let stagingRows = $derived.by(() => {
        const seen = new Set<string>();
        const rows: SrsCard[] = [];
        for (const c of stagingCards) {
            if (seen.has(c.word)) continue;
            seen.add(c.word);
            rows.push(c);
        }
        return rows;
    });

    $effect(() => watchCardsDb(loadStaging));

    async function loadStaging() {
        const res = await browser.runtime.sendMessage({ type: "GET_STAGING" });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        stagingCards = cards;
        const words = [...new Set(cards.map((c) => c.word))];
        entriesByWord = await buildEntryMap(words);
    }

    async function promoteCard(word: string) {
        await browser.runtime.sendMessage({ type: "PROMOTE_CARD", payload: { word } });
        stagingCards = stagingCards.filter((c) => c.word !== word);
        onstagingchange(stagingRows.length);
    }

    async function promoteAll() {
        await browser.runtime.sendMessage({ type: "PROMOTE_ALL" });
        stagingCards = [];
        onstagingchange(0);
    }

    function addedLabel(ms: number): string {
        const diff = Date.now() - ms;
        const days = Math.floor(diff / 86_400_000);
        if (days === 0) return "today";
        if (days === 1) return "1d ago";
        return `${days}d ago`;
    }
</script>

<div class="word-list-header">
    <span class="word-count">{stagingRows.length} new word{stagingRows.length !== 1 ? "s" : ""}</span>
    {#if stagingRows.length > 0}
        <button class="btn-promote-all" onclick={promoteAll}>Add all to review</button>
    {/if}
</div>

{#if stagingRows.length === 0}
    <div class="empty"><p>No new words yet.</p></div>
{:else}
    <table class="word-table">
        <thead>
            <tr><th>Word</th><th>Reading</th><th>Meaning</th><th>Added</th><th></th></tr>
        </thead>
        <tbody>
            {#each stagingRows as card (card.word)}
                {@const entry = entriesByWord[card.word] ?? null}
                <tr>
                    <td class="td-word">{card.word}</td>
                    <td class="td-reading">{readingOf(entry)}</td>
                    <td class="td-meaning">
                        {#if entry}{meaningOf(entry)}{:else}<span class="td-missing">not in dictionary</span>{/if}
                    </td>
                    <td>{addedLabel(card.added_ms)}</td>
                    <td><button class="btn-promote" onclick={() => promoteCard(card.word)}>Add to review</button></td>
                </tr>
            {/each}
        </tbody>
    </table>
{/if}

<style>
    .word-list-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 12px;
    }
    .word-count {
        color: var(--subtext);
        font-size: 13px;
    }

    .empty {
        text-align: center;
        padding: 32px;
        color: var(--subtext);
    }

    .word-table {
        width: 100%;
        border-collapse: collapse;
        font-size: 13px;
    }
    .word-table th {
        text-align: left;
        padding: 6px 8px;
        border-bottom: 1px solid var(--border);
        color: var(--subtext);
        font-weight: 500;
    }
    .word-table td {
        padding: 6px 8px;
        border-bottom: 1px solid var(--border);
        vertical-align: top;
    }
    .word-table tr:last-child td {
        border-bottom: none;
    }

    .td-word    { font-size: 18px; color: var(--blue);  }
    .td-reading { color: var(--green); }
    .td-meaning { color: var(--text); max-width: 180px; }

    .btn-promote {
        background: none;
        border: 1px solid var(--green);
        border-radius: 4px;
        color: var(--green);
        cursor: pointer;
        font-size: 11px;
        padding: 2px 8px;
        white-space: nowrap;
    }
    .btn-promote:hover {
        background: var(--surface);
    }

    .btn-promote-all {
        background: var(--accent);
        border: none;
        border-radius: 6px;
        color: var(--bg);
        cursor: pointer;
        font-size: 12px;
        font-weight: 600;
        padding: 4px 14px;
        transition: opacity 0.15s;
    }
    .btn-promote-all:hover {
        opacity: 0.85;
    }
</style>
