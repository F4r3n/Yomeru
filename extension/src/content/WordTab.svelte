<script lang="ts">
    import type { WordEntry } from "../shared/types.ts";

    let {
        entries,
        buttonStates,
        onadd,
    }: {
        entries: WordEntry[];
        buttonStates: Record<string, "idle" | "added" | "existing">;
        onadd: (word: string) => void;
    } = $props();

    function headword(e: WordEntry): string {
        return e.kanji_forms[0]?.text ?? e.reading_forms[0]?.text ?? "";
    }
    function reading(e: WordEntry): string {
        return e.reading_forms[0]?.text ?? "";
    }
</script>

{#each entries.slice(0, 4) as entry, i (entry.sequence)}
    {#if i > 0}<hr class="jp-divider" />{/if}
    {@const hw = headword(entry)}
    {@const rdg = reading(entry)}
    {@const btnState = buttonStates[hw] ?? "idle"}
    <div class="jp-entry">
        <div class="jp-header">
            <span class="jp-word">{hw}</span>
            {#if rdg && rdg !== hw}
                <span class="jp-reading">【{rdg}】</span>
            {/if}
            <span class="jp-pos-group">
                {#each entry.senses[0]?.pos ?? [] as pos}
                    <span class="jp-pos">{pos}</span>
                {/each}
            </span>
        </div>
        <div class="jp-senses">
            {#each entry.senses.slice(0, 3) as sense, si}
                {@const g = sense.glosses.map((g) => g.text).join("; ")}
                {#if g}
                    <div class="jp-gloss">
                        <span class="jp-num">{si + 1}.</span>{g}
                    </div>
                {/if}
            {/each}
        </div>
        <button
            class="jp-add-btn"
            disabled={btnState !== "idle"}
            onclick={() => onadd(hw)}
        >
            {#if btnState === "idle"}+ Add to SRS
            {:else if btnState === "added"}Added!
            {:else}In SRS{/if}
        </button>
    </div>
{/each}
