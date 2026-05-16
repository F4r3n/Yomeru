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
        buttonStates: Record<string, "idle" | "added" | "existing">;
        onadd: (word: string) => void;
        limit?: number;
    } = $props();

    function headword(e: WordEntry): string {
        return e.kanji_forms[0]?.text ?? e.reading_forms[0]?.text ?? "";
    }
</script>

{#each entries.slice(0, limit) as entry, i (entry.sequence)}
    {#if i > 0}<hr class="jp-divider" />{/if}
    <EntryCard
        {entry}
        btnState={buttonStates[headword(entry)] ?? "idle"}
        {onadd}
    />
{/each}
