<script lang="ts">
    // The grammar points the hovered text starts with, at a glance: the
    // explanation stays on the Lookup page. Styles live in popup.css.ts,
    // which the popup's shadow root loads.
    import type { GrammarPopupEntry, WordStatus } from "../shared/types.ts";
    import { MAX_PRIORITY } from "../shared/types.ts";

    let { entries }: { entries: GrammarPopupEntry[] } = $props();

    // Deck standing per point, by card sequence. Missing = not fetched yet.
    let statuses = $state<Record<number, WordStatus>>({});
    let busy = $state<number | null>(null);

    $effect(() => {
        const sequences = entries.map((e) => e.seq);
        statuses = {};
        browser.runtime
            .sendMessage({ type: "GET_WORD_STATUS", payload: { sequences } })
            .then((r: { statuses: Record<number, WordStatus> }) => {
                // Only if the popup still shows these points.
                if (entries.map((e) => e.seq).join(",") === sequences.join(",")) {
                    statuses = r.statuses;
                }
            })
            .catch(() => {});
    });

    /** Adds the point (staging, like a word), or bumps its priority once staged. */
    async function add(seq: number, staged: boolean) {
        busy = seq;
        try {
            if (staged) {
                const st = (await browser.runtime.sendMessage({
                    type: "BUMP_PRIORITY",
                    payload: { sequence: seq },
                })) as WordStatus;
                statuses = { ...statuses, [seq]: st };
            } else {
                await browser.runtime.sendMessage({ type: "ADD_GRAMMAR", payload: { sequence: seq } });
                const fresh = await browser.runtime.sendMessage({
                    type: "GET_WORD_STATUS",
                    payload: { sequences: [seq] },
                });
                statuses = { ...statuses, ...fresh.statuses };
            }
        } finally {
            busy = null;
        }
    }
</script>

<div class="jp-grammar">
    {#each entries as entry (entry.id)}
        {@const st = statuses[entry.seq]}
        <div class="jp-grammar-entry">
            <div class="jp-header">
                <span class="jp-word jp-grammar-term">{entry.pattern}</span>
                {#if entry.jlpt}
                    <span class="jp-freq">{entry.jlpt}</span>
                {/if}
            </div>
            {#if entry.meaning}
                <div class="jp-grammar-meaning">{entry.meaning}</div>
            {/if}
            {#if !st}
                <button class="jp-add-btn" disabled>…</button>
            {:else if st.status === "none"}
                <button class="jp-add-btn" disabled={busy === entry.seq} onclick={() => add(entry.seq, false)}>
                    + Add to deck
                </button>
            {:else if st.status === "staging" && st.priority >= MAX_PRIORITY}
                <button class="jp-add-btn" disabled>★ Staged · Priority maxed ({MAX_PRIORITY})</button>
            {:else if st.status === "staging"}
                <button class="jp-add-btn" disabled={busy === entry.seq} onclick={() => add(entry.seq, true)}>
                    ☆ Staged · +Priority (★{st.priority})
                </button>
            {:else}
                <button class="jp-add-btn" disabled>
                    {st.status === "graduated" ? "✓ Graduated" : "✓ In deck"}
                </button>
            {/if}
            {#each entry.formation.slice(0, 2) as line}
                <div class="jp-grammar-structure">{line}</div>
            {/each}
            {#if entry.examples.length > 0}
                <div class="jp-grammar-examples">
                    {#each entry.examples as ex}
                        <div class="jp-corpus-ex">
                            <div class="jp-corpus-jp">
                                {#each ex.ja as span}
                                    {#if "highlight" in span}<mark class="jp-corpus-mark">{span.highlight.text}</mark>
                                    {:else}{span.text.text}{/if}
                                {/each}
                            </div>
                            {#if ex.en}
                                <div class="jp-corpus-en">{ex.en}</div>
                            {/if}
                        </div>
                    {/each}
                </div>
            {/if}
        </div>
    {/each}
    <div class="jp-grammar-credit">Full explanation on the Lookup page</div>
</div>
