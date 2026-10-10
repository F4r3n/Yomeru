<script lang="ts">
    import type { WordEntry, WordStatus } from "../shared/types.ts";
    import EntryCard from "./EntryCard.svelte";

    let {
        entries,
        statuses,
        onadd,
        onpriority,
        onreset,
        limit = 4,
    }: {
        entries: WordEntry[];
        statuses: Record<number, WordStatus>;
        onadd: (entry: WordEntry) => Promise<void>;
        onpriority: (entry: WordEntry) => Promise<void>;
        onreset: (entry: WordEntry) => Promise<void>;
        limit?: number;
    } = $props();
</script>

{#each entries.slice(0, limit) as entry, i (entry.sequence)}
    {#if i > 0}<hr class="jp-divider" />{/if}
    <EntryCard
        {entry}
        status={statuses[entry.sequence]}
        {onadd}
        {onpriority}
        {onreset}
    />
{/each}
