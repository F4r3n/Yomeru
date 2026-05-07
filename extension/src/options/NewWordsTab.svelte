<script lang="ts">
    import type { SrsCard, SrsSettings } from "../shared/types.ts";
    import { DEFAULT_SETTINGS } from "../shared/types.ts";

    let { onstagingchange }: { onstagingchange: (n: number) => void } = $props();

    let stagingCards = $state<SrsCard[]>([]);
    let settings = $state<SrsSettings>({ ...DEFAULT_SETTINGS });

    $effect(() => {
        loadStaging();
    });

    async function loadStaging() {
        const [stagingRes, settingsRes] = await Promise.all([
            browser.runtime.sendMessage({ type: "GET_STAGING" }),
            browser.runtime.sendMessage({ type: "GET_SETTINGS" }),
        ]);
        stagingCards = (stagingRes as { cards: SrsCard[] }).cards ?? [];
        settings = (settingsRes as SrsSettings) ?? { ...DEFAULT_SETTINGS };
    }

    async function promoteCard(word: string) {
        await browser.runtime.sendMessage({ type: "PROMOTE_CARD", payload: { word } });
        stagingCards = stagingCards.filter((c) => c.word !== word);
        onstagingchange(stagingCards.length);
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

{#if settings.maxStagingSize > 0 && stagingCards.length >= settings.maxStagingSize}
    <div class="warning-staging-full">
        Staging list is full ({settings.maxStagingSize} words). New words cannot be added until you promote some.
    </div>
{/if}

<div class="word-list-header">
    <span class="word-count">{stagingCards.length} new word{stagingCards.length !== 1 ? "s" : ""}</span>
    {#if stagingCards.length > 0}
        <button class="btn-promote-all" onclick={promoteAll}>Add all to review</button>
    {/if}
</div>

{#if stagingCards.length === 0}
    <div class="empty"><p>No new words yet.</p></div>
{:else}
    <table class="word-table">
        <thead>
            <tr><th>Word</th><th>Reading</th><th>Meaning</th><th>Added</th><th></th></tr>
        </thead>
        <tbody>
            {#each stagingCards as card (card.word)}
                <tr>
                    <td class="td-word">{card.word}</td>
                    <td class="td-reading">{card.reading}</td>
                    <td class="td-meaning">{card.meaning_en}</td>
                    <td>{addedLabel(card.added_ms)}</td>
                    <td><button class="btn-promote" onclick={() => promoteCard(card.word)}>Add to review</button></td>
                </tr>
            {/each}
        </tbody>
    </table>
{/if}

<style>
    .warning-staging-full {
        background: var(--yellow);
        color: var(--bg);
        border-radius: 6px;
        font-size: 12px;
        padding: 6px 12px;
        margin-bottom: 10px;
    }

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
