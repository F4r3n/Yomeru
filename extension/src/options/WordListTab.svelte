<script lang="ts">
    import type { SrsCard, WordEntry } from "../shared/types.ts";
    import { buildEntryMap, readingOf, meaningOf } from "./dict-lookup.ts";
    import { watchCardsDb } from "./db-watch.ts";

    let allCards = $state<SrsCard[]>([]);
    let entriesByWord = $state<Record<string, WordEntry | null>>({});
    let searchQuery = $state("");

    let filteredCards = $derived(
        allCards
            .filter((c) => {
                if (!searchQuery) return true;
                const q = searchQuery.toLowerCase();
                const e = entriesByWord[c.word] ?? null;
                return (
                    c.word.includes(q) ||
                    readingOf(e).includes(q) ||
                    meaningOf(e).toLowerCase().includes(q)
                );
            })
            .sort((a, b) => {
                const da = a.status === "staging" ? Infinity : a.due_ms;
                const db = b.status === "staging" ? Infinity : b.due_ms;
                return da - db;
            }),
    );

    $effect(() => watchCardsDb(loadWords));

    async function loadWords() {
        const res = await browser.runtime.sendMessage({ type: "GET_ALL_CARDS" });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        allCards = cards;
        entriesByWord = await buildEntryMap(cards.map((c) => c.word));
    }

    async function deleteCard(word: string) {
        await browser.runtime.sendMessage({ type: "DELETE_CARD", payload: { word } });
        allCards = allCards.filter((c) => c.word !== word);
    }

    function dueLabel(ms: number): string {
        const diff = ms - Date.now();
        if (diff <= 0) return "Due now";
        const mins = Math.round(diff / 60_000);
        if (mins < 60) return `${mins}m`;
        const h = Math.round(diff / 3_600_000);
        return h < 24 ? `${h}h` : `${Math.round(diff / 86_400_000)}d`;
    }

    function dueClass(ms: number): string {
        const diff = ms - Date.now();
        if (diff <= 0) return "overdue";
        if (diff < 86_400_000) return "today";
        return "future";
    }
</script>

<div class="word-list-header">
    <span class="word-count">{filteredCards.length} words</span>
    <input
        class="word-search"
        type="search"
        placeholder="Search…"
        bind:value={searchQuery}
    />
</div>

<div class="word-table-scroll">
<table class="word-table">
    <thead>
        <tr><th>Word</th><th>Reading</th><th>Meaning</th><th>Status</th><th>Due</th><th></th></tr>
    </thead>
    <tbody>
        {#each filteredCards as card (card.word)}
            {@const entry = entriesByWord[card.word] ?? null}
            <tr>
                <td class="td-word">{card.word}</td>
                <td class="td-reading">{readingOf(entry)}</td>
                <td class="td-meaning">
                    {#if entry}{meaningOf(entry)}{:else}<span class="td-missing">not in dictionary</span>{/if}
                </td>
                <td><span class="status-badge {card.status}">{card.status === "staging" ? "new" : card.status}</span></td>
                <td class="td-due {card.status === 'staging' ? '' : dueClass(card.due_ms)}">{card.status === "staging" ? "—" : dueLabel(card.due_ms)}</td>
                <td><button class="btn-delete" onclick={() => deleteCard(card.word)}>Delete</button></td>
            </tr>
        {/each}
    </tbody>
</table>
</div>

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
    .word-search {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        font-size: 13px;
        padding: 4px 10px;
        outline: none;
        width: 180px;
    }
    .word-search:focus {
        border-color: var(--accent);
    }

    .word-table-scroll {
        max-height: 400px;
        overflow-y: auto;
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
    .td-missing { color: var(--red); font-style: italic; }

    .td-due.overdue { color: var(--red);     }
    .td-due.today   { color: var(--yellow);  }
    .td-due.future  { color: var(--subtext); }

    .status-badge {
        font-size: 11px;
        padding: 1px 6px;
        border-radius: 10px;
    }
    .status-badge.staging { background: var(--yellow); color: var(--bg); }
    .status-badge.active  { background: var(--green);  color: var(--bg); }

    .btn-delete {
        background: none;
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--red);
        cursor: pointer;
        font-size: 11px;
        padding: 2px 8px;
    }
    .btn-delete:hover {
        background: var(--surface);
    }
</style>
