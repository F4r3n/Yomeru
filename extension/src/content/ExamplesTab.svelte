<script lang="ts">
    import type { ExampleEntry } from "../shared/types.ts";

    let {
        examples,
        fetched,
        word = "",
    }: {
        examples: ExampleEntry[];
        fetched: boolean;
        word?: string;
    } = $props();

    function splitAtWord(text: string, w: string): [string, string, string] | null {
        if (!w) return null;
        const idx = text.indexOf(w);
        if (idx === -1) return null;
        return [text.slice(0, idx), w, text.slice(idx + w.length)];
    }
</script>

{#if examples.length > 0}
    <div class="jp-corpus">
        {#each examples as ex}
            {@const parts = splitAtWord(ex.japanese, word)}
            <div class="jp-corpus-ex">
                <div class="jp-corpus-jp">
                    {#if parts}
                        {parts[0]}<mark class="jp-corpus-mark">{parts[1]}</mark>{parts[2]}
                    {:else}
                        {ex.japanese}
                    {/if}
                </div>
                <div class="jp-corpus-en">{ex.english}</div>
            </div>
        {/each}
    </div>
{:else if fetched}
    <div class="jp-corpus-empty">No examples found.</div>
{/if}
