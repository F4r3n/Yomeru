<script lang="ts">
    import type { WordEntry } from "../shared/types.ts";
    import { preferredHeadword, frequencyLabel } from "../shared/dict.ts";

    let {
        entry,
        btnState,
        onadd,
    }: {
        entry: WordEntry;
        btnState: "idle" | "added" | "existing";
        onadd: (entry: WordEntry) => void;
    } = $props();

    let hw = $derived(preferredHeadword(entry));
    let rdg = $derived(entry.reading_forms[0]?.text ?? "");
    // When the title is the kana reading (kana-preferred entry), show the kanji
    // writing alongside it in smaller text instead of the reading.
    let kanaTitled = $derived(hw === rdg);
    let subKanji = $derived(kanaTitled ? (entry.kanji_forms[0]?.text ?? "") : "");
    let freq = $derived(frequencyLabel(entry));
</script>

<div class="jp-entry">
    <div class="jp-header">
        <span class="jp-word">{hw}</span>
        {#if subKanji}
            <span class="jp-kanji-sub">{subKanji}</span>
        {:else if rdg && rdg !== hw}
            <span class="jp-reading">【{rdg}】</span>
        {/if}
        {#if freq}
            <span class="jp-freq">{freq}</span>
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
        onclick={() => onadd(entry)}
    >
        {#if btnState === "idle"}+ Add to SRS
        {:else if btnState === "added"}Added!
        {:else}In SRS{/if}
    </button>
</div>
