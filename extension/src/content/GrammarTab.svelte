<script lang="ts">
    // The grammar points the hovered text starts with, at a glance: the full
    // sources stay on the Lookup page. Styles live in popup.css.ts, which the
    // popup's shadow root loads.
    import type { GrammarPopupEntry, GrammarSpan } from "../shared/types.ts";

    let { entries }: { entries: GrammarPopupEntry[] } = $props();

    /** Unwraps a span into what to render. */
    function part(span: GrammarSpan):
        | { kind: "text" | "highlight" | "label"; text: string }
        | { kind: "ruby"; text: string; ruby: string }
        | { kind: "br" } {
        if (span === "lineBreak") return { kind: "br" };
        if ("highlight" in span) return { kind: "highlight", text: span.highlight.text };
        if ("label" in span) return { kind: "label", text: span.label.text };
        if ("ruby" in span) return { kind: "ruby", ...span.ruby };
        // A link to another headword: plain text here.
        if ("link" in span) return { kind: "text", text: span.link.text };
        return { kind: "text", text: span.text.text };
    }
</script>

<div class="jp-grammar">
    {#each entries as entry}
        <div class="jp-grammar-entry">
            <div class="jp-header">
                <span class="jp-word jp-grammar-term">{entry.term}</span>
                {#if entry.reading && entry.reading !== entry.term}
                    <span class="jp-reading">{entry.reading}</span>
                {/if}
                {#if entry.jlpt}
                    <span class="jp-freq">{entry.jlpt}</span>
                {/if}
            </div>
            {#if entry.meaning}
                <div class="jp-grammar-meaning">{entry.meaning}</div>
            {/if}
            {#if entry.structure}
                <div class="jp-grammar-structure">{entry.structure}</div>
            {/if}
            {#if entry.examples.length > 0}
                <div class="jp-grammar-examples">
                    {#each entry.examples as ex}
                        <div class="jp-corpus-ex">
                            <div class="jp-corpus-jp">
                                {#each ex.ja.map(part) as p}
                                    {#if p.kind === "br"}<br />
                                    {:else if p.kind === "highlight"}<mark class="jp-corpus-mark">{p.text}</mark>
                                    {:else if p.kind === "label"}<strong>{p.text}</strong>
                                    {:else if p.kind === "ruby"}<ruby>{p.text}<rt>{p.ruby}</rt></ruby>
                                    {:else}{p.text}{/if}
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
    <div class="jp-grammar-credit">Bee's Ultimate Grammar Dictionary · full explanations on the Lookup page</div>
</div>
