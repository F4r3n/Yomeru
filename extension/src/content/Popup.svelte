<script lang="ts">
    import { popupStore } from "./popup-store";
    import type { WordEntry, ExampleEntry, Sense } from "../shared/types.ts";
    import { srsWordAdded } from "./srs-highlighter";
    import WordTab from "./WordTab.svelte";
    import KanjiTab from "./KanjiTab.svelte";
    import ExamplesTab from "./ExamplesTab.svelte";

    let activeTab = $state<"word" | "kanji" | "examples">("word");
    let buttonStates = $state<Record<string, "idle" | "added" | "existing">>({});
    let corpusExamples = $state<ExampleEntry[]>([]);
    let examplesFetched = $state(false);

    let entriesKey = $derived(
        $popupStore.entries.map((e: WordEntry) => e.sequence).join(","),
    );

    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-unused-expressions
        entriesKey;
        buttonStates = {};
        activeTab = "word";
        corpusExamples = [];
        examplesFetched = false;
    });

    function position(node: HTMLElement, params: { wx1: number; wx2: number; wy1: number; wy2: number }) {
        function recompute({ wx1, wy1, wy2 }: { wx1: number; wx2: number; wy1: number; wy2: number }) {
            // Reset to measure natural width before committing position.
            node.style.left = "0px";
            node.style.top = "0px";
            node.style.bottom = "auto";
            const spaceBelow = window.innerHeight - 8 - wy2 - 4;
            const spaceAbove = wy1 - 4 - 8;
            const useBelow = spaceBelow >= spaceAbove;
            node.style.maxHeight = `${Math.max(0, useBelow ? spaceBelow : spaceAbove)}px`;
            // Read width only (stable, independent of content rerender timing).
            const w = node.getBoundingClientRect().width;
            node.style.left = `${Math.max(8, Math.min(wx1, window.innerWidth - w - 8))}px`;
            // Use bottom-anchor when above so we never need to know popup height.
            if (useBelow) {
                node.style.top = `${wy2 + 4}px`;
                node.style.bottom = "auto";
            } else {
                node.style.top = "auto";
                node.style.bottom = `${window.innerHeight - wy1 + 4}px`;
            }
        }
        recompute(params);
        return { update: recompute };
    }

    function openExamples() {
        activeTab = "examples";
        if (examplesFetched) return;
        const hw =
            $popupStore.entries[0]?.kanji_forms[0]?.text ??
            $popupStore.entries[0]?.reading_forms[0]?.text ?? "";
        if (!hw) return;
        examplesFetched = true;
        browser.runtime
            .sendMessage({ type: "GET_EXAMPLES", payload: { word: hw } })
            .then((res: { entries: ExampleEntry[] }) => {
                corpusExamples = res?.entries ?? [];
            })
            .catch(() => {});
    }

    async function addToSrs(word: string, rdg: string, meaning: string, senses: Sense[]) {
        const res = await browser.runtime.sendMessage({
            type: "ADD_WORD",
            payload: { word, reading: rdg, meaning_en: meaning, senses },
        });
        buttonStates = {
            ...buttonStates,
            [word]: res.existing ? "existing" : "added",
        };
        if (!res.existing) srsWordAdded(word);
    }
</script>

{#if $popupStore.visible && $popupStore.entries.length > 0}
    <div class="jp-popup" use:position={{ wx1: $popupStore.wx1, wx2: $popupStore.wx2, wy1: $popupStore.wy1, wy2: $popupStore.wy2 }}>
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

        <div class="jp-tabs">
            <button
                class="jp-tab"
                class:jp-tab--active={activeTab === "word"}
                onclick={() => (activeTab = "word")}>Word</button
            >
            {#if $popupStore.kanjiEntries.length > 0}
                <button
                    class="jp-tab"
                    class:jp-tab--active={activeTab === "kanji"}
                    onclick={() => (activeTab = "kanji")}>Kanji</button
                >
            {/if}
            <button
                class="jp-tab"
                class:jp-tab--active={activeTab === "examples"}
                onclick={openExamples}>Examples</button
            >
        </div>

        {#if activeTab === "word"}
            <WordTab
                entries={$popupStore.entries}
                {buttonStates}
                onadd={addToSrs}
            />
        {:else if activeTab === "kanji"}
            <KanjiTab kanjiEntries={$popupStore.kanjiEntries} />
        {:else}
            <ExamplesTab examples={corpusExamples} fetched={examplesFetched} />
        {/if}
    </div>
{/if}
