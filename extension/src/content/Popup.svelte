<script lang="ts">
    import { popupStore } from "./popup-store";
    import type { WordEntry } from "../shared/types.ts";

    // Tracks "+ Add to SRS" button state per word, reset on each new lookup.
    let buttonStates = $state<Record<string, "idle" | "added" | "existing">>(
        {},
    );

    let entriesKey = $derived(
        $popupStore.entries.map((e) => e.sequence).join(","),
    );

    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-unused-expressions
        entriesKey;
        buttonStates = {};
    });

    function headword(e: WordEntry): string {
        return e.kanji_forms[0]?.text ?? e.reading_forms[0]?.text ?? "";
    }

    function reading(e: WordEntry): string {
        return e.reading_forms[0]?.text ?? "";
    }

    function firstGloss(e: WordEntry): string {
        return e.senses[0]?.glosses.find((g) => g.lang === "eng")?.text ?? "";
    }

    // Svelte action: viewport-aware popup positioning.
    function position(node: HTMLElement, params: { x: number; y: number }) {
        function recompute({ x, y }: { x: number; y: number }) {
            node.style.left = "0px";
            node.style.top = "0px";
            const rect = node.getBoundingClientRect();
            let left = x + 12;
            let top = y + 20;
            if (left + rect.width > window.innerWidth - 8)
                left = x - rect.width - 12;
            if (top + rect.height > window.innerHeight - 8)
                top = y - rect.height - 8;
            node.style.left = `${Math.max(8, left)}px`;
            node.style.top = `${Math.max(8, top)}px`;
        }
        recompute(params);
        return { update: recompute };
    }

    async function addToSrs(word: string, rdg: string, meaning: string) {
        const res = await browser.runtime.sendMessage({
            type: "ADD_WORD",
            payload: { word, reading: rdg, meaning_en: meaning },
        });
        buttonStates = {
            ...buttonStates,
            [word]: res.existing ? "existing" : "added",
        };
    }
</script>

{#if $popupStore.visible && $popupStore.entries.length > 0}
    <div class="jp-popup" use:position={{ x: $popupStore.x, y: $popupStore.y }}>
        {#key entriesKey}
            <div class="jp-pin-ring" aria-hidden="true">
                {#if $popupStore.pinned}
                    <svg viewBox="0 0 18 18" width="14" height="14">
                        <circle class="jp-pin-dot" cx="9" cy="9" r="5" />
                    </svg>
                {:else}
                    <svg viewBox="0 0 18 18" width="14" height="14">
                        <circle class="jp-ring-track" cx="9" cy="9" r="7" />
                        <circle class="jp-ring-fill" cx="9" cy="9" r="7" />
                    </svg>
                {/if}
            </div>
        {/key}
        {#each $popupStore.entries.slice(0, 4) as entry, i (entry.sequence)}
            {#if i > 0}<hr class="jp-divider" />{/if}
            {@const hw = headword(entry)}
            {@const rdg = reading(entry)}
            {@const gloss = firstGloss(entry)}
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
                        {@const g = sense.glosses
                            .filter((g) => g.lang === "eng")
                            .map((g) => g.text)
                            .join("; ")}
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
                    onclick={() =>
                        btnState === "idle" && addToSrs(hw, rdg, gloss)}
                >
                    {#if btnState === "idle"}+ Add to SRS
                    {:else if btnState === "added"}Added!
                    {:else}Already added{/if}
                </button>
            </div>
        {/each}
    </div>
{/if}
