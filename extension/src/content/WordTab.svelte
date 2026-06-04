<script lang="ts">
    import type { WordEntry } from "../shared/types.ts";
    import EntryCard from "./EntryCard.svelte";

    let {
        entries,
        buttonStates,
        onadd,
        limit = 4,
    }: {
        entries: WordEntry[];
        buttonStates: Record<number, "idle" | "added" | "existing">;
        onadd: (entry: WordEntry) => void;
        limit?: number;
    } = $props();
</script>

{#each entries.slice(0, limit) as entry, i (entry.sequence)}
    {#if i > 0}<hr class="jp-divider" />{/if}
    <EntryCard
        {entry}
        btnState={buttonStates[entry.sequence] ?? "idle"}
        {onadd}
    />
{/each}
