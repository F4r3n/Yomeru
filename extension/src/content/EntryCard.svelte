<script lang="ts">
    import type { WordEntry } from "../shared/types.ts";

    let {
        entry,
        btnState,
        onadd,
    }: {
        entry: WordEntry;
        btnState: "idle" | "added" | "existing";
        onadd: (word: string) => void;
    } = $props();

    let hw = $derived(entry.kanji_forms[0]?.text ?? entry.reading_forms[0]?.text ?? "");
    let rdg = $derived(entry.reading_forms[0]?.text ?? "");
</script>

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
