<script lang="ts">
    import type { SrsCard, WordEntry } from "../shared/types.ts";
    import { MS_PER_DAY } from "../shared/types.ts";
    import { buildEntryMap, readingOf, meaningOf } from "./dict-lookup.ts";
    import { watchCardsDb } from "./db-watch.ts";
    import { isRomaji, romajiToHiragana } from "./romaji.ts";

    type DirState = { status: SrsCard["status"]; due_ms: number } | null;
    type WordRow = {
        word: string;
        recognition: DirState;
        recall: DirState;
        // Earliest active due across both directions; Infinity if none active.
        next_active_due_ms: number;
        // Worst-case status across siblings: "staging" if any sibling is
        // staging, else "active". Drives the row's sort bucket.
        agg_status: SrsCard["status"];
        added_ms: number;
    };

    let allCards = $state<SrsCard[]>([]);
    let entriesByWord = $state<Record<string, WordEntry | null>>({});
    let searchQuery = $state("");

    // Aggregate both direction siblings into one row per word.
    // added_ms is taken from the first sibling encountered — by construction
    // both siblings are inserted with the same value (ADD_WORD spreads one
    // `base` into both, and the v3 migration copies recognition.added_ms to
    // recall), so the choice is moot.
    let wordRows = $derived.by(() => {
        const byWord = new Map<string, WordRow>();
        for (const c of allCards) {
            const existing = byWord.get(c.word);
            const dirState: DirState = { status: c.status, due_ms: c.due_ms };
            if (!existing) {
                byWord.set(c.word, {
                    word: c.word,
                    recognition: c.direction === "recognition" ? dirState : null,
                    recall: c.direction === "recall" ? dirState : null,
                    next_active_due_ms: c.status === "active" ? c.due_ms : Infinity,
                    agg_status: c.status,
                    added_ms: c.added_ms,
                });
            } else {
                if (c.direction === "recognition") existing.recognition = dirState;
                else existing.recall = dirState;
                if (c.status === "active") {
                    existing.next_active_due_ms = Math.min(existing.next_active_due_ms, c.due_ms);
                }
                if (c.status === "staging") existing.agg_status = "staging";
            }
        }
        return [...byWord.values()];
    });

    let filteredRows = $derived(
        wordRows
            .filter((r) => {
                if (!searchQuery) return true;
                const q = searchQuery.toLowerCase();
                const kana = isRomaji(searchQuery.trim()) ? romajiToHiragana(searchQuery.trim()) : "";
                const e = entriesByWord[r.word] ?? null;
                return (
                    r.word.includes(q) ||
                    readingOf(e).includes(q) ||
                    meaningOf(e).toLowerCase().includes(q) ||
                    (kana !== "" && (r.word.includes(kana) || readingOf(e).includes(kana)))
                );
            })
            .sort((a, b) => a.next_active_due_ms - b.next_active_due_ms),
    );

    function isMixed(row: WordRow): boolean {
        return (
            row.recognition !== null &&
            row.recall !== null &&
            row.recognition.status !== row.recall.status
        );
    }

    $effect(() => watchCardsDb(loadWords));

    async function loadWords() {
        const res = await browser.runtime.sendMessage({ type: "GET_ALL_CARDS" });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        allCards = cards;
        const words = [...new Set(cards.map((c) => c.word))];
        entriesByWord = await buildEntryMap(words);
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
        return h < 24 ? `${h}h` : `${Math.round(diff / MS_PER_DAY)}d`;
    }

    function dueClass(ms: number): string {
        const diff = ms - Date.now();
        if (diff <= 0) return "overdue";
        if (diff < MS_PER_DAY) return "today";
        return "future";
    }
</script>

<div class="word-list-header">
    <span class="word-count">{filteredRows.length} words</span>
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
        {#each filteredRows as row (row.word)}
            {@const entry = entriesByWord[row.word] ?? null}
            {@const mixed = isMixed(row)}
            <tr>
                <td class="td-word">{row.word}</td>
                <td class="td-reading">{readingOf(entry)}</td>
                <td class="td-meaning">
                    {#if entry}{meaningOf(entry)}{:else}<span class="td-missing">not in dictionary</span>{/if}
                </td>
                <td>
                    {#if mixed}
                        <span class="status-pair" title="Recognition and recall are in different states">
                            {#if row.recognition}
                                <span class="status-mini {row.recognition.status}" title="Recognition: {row.recognition.status}">R</span>
                            {/if}
                            {#if row.recall}
                                <span class="status-mini {row.recall.status}" title="Recall: {row.recall.status}">L</span>
                            {/if}
                        </span>
                    {:else}
                        <span class="status-badge {row.agg_status}">{row.agg_status === "staging" ? "new" : row.agg_status}</span>
                    {/if}
                </td>
                <td class="td-due {row.next_active_due_ms === Infinity ? '' : dueClass(row.next_active_due_ms)}">
                    {row.next_active_due_ms === Infinity ? "—" : dueLabel(row.next_active_due_ms)}
                </td>
                <td><button class="btn-delete" onclick={() => deleteCard(row.word)}>Delete</button></td>
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

    .status-pair {
        display: inline-flex;
        gap: 3px;
    }
    .status-mini {
        font-size: 10px;
        font-weight: 700;
        width: 16px;
        height: 16px;
        line-height: 16px;
        border-radius: 50%;
        text-align: center;
        color: var(--bg);
    }
    .status-mini.staging { background: var(--yellow); }
    .status-mini.active  { background: var(--green);  }

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
