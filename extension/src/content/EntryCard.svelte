<script lang="ts">
    import type { WordEntry, WordStatus } from "../shared/types.ts";
    import { MAX_PRIORITY } from "../shared/types.ts";
    import { preferredHeadword, frequencyLabel } from "../shared/dict.ts";

    let {
        entry,
        status,
        onadd,
        onpriority,
        onreset,
    }: {
        entry: WordEntry;
        /** Undefined while the background is still being asked. */
        status: WordStatus | undefined;
        onadd: (entry: WordEntry) => Promise<void>;
        onpriority: (entry: WordEntry) => Promise<void>;
        onreset: (entry: WordEntry) => Promise<void>;
    } = $props();

    let busy = $state(false);
    // Reset wipes the word's review history, so it takes a second click.
    // An inline confirm rather than `window.confirm`, which some pages block
    // and which would freeze the page the popup sits on.
    let confirmingReset = $state(false);

    async function run(action: (entry: WordEntry) => Promise<void>) {
        busy = true;
        try {
            await action(entry);
        } finally {
            busy = false;
        }
    }

    function onResetClick() {
        if (!confirmingReset) {
            confirmingReset = true;
            return;
        }
        confirmingReset = false;
        void run(onreset);
    }

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
    {#if !status}
        <button class="jp-add-btn" disabled>…</button>
    {:else if status.status === "none"}
        <button class="jp-add-btn" disabled={busy} onclick={() => run(onadd)}>
            + Add to SRS
        </button>
    {:else if status.status === "staging"}
        {#if status.priority >= MAX_PRIORITY}
            <button class="jp-add-btn" disabled>★ Staged · Priority maxed ({MAX_PRIORITY})</button>
        {:else}
            <button class="jp-add-btn" disabled={busy} onclick={() => run(onpriority)}>
                ☆ Staged · +Priority (★{status.priority})
            </button>
        {/if}
    {:else}
        <button
            class="jp-add-btn"
            class:jp-add-btn--danger={confirmingReset}
            disabled={busy}
            onclick={onResetClick}
            onmouseleave={() => (confirmingReset = false)}
        >
            {#if confirmingReset}Reset progress? Click again
            {:else if status.status === "graduated"}✓ Graduated · Reset
            {:else}✓ In SRS · Reset{/if}
        </button>
    {/if}
</div>
