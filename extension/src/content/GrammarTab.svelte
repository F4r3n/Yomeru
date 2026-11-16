<script lang="ts">
    // The grammar points the hovered text starts with, at a glance: the
    // explanation stays on the Lookup page. Styles live in popup.css.ts,
    // which the popup's shadow root loads.
    import type { GrammarPopupEntry } from "../shared/types.ts";

    let { entries }: { entries: GrammarPopupEntry[] } = $props();
</script>

<div class="jp-grammar">
    {#each entries as entry (entry.id)}
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
