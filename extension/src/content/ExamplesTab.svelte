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

<style>
    .jp-corpus { display: flex; flex-direction: column; gap: 12px; padding: 4px 0; }
    .jp-corpus-ex { display: flex; flex-direction: column; gap: 3px; }
    .jp-corpus-jp { font-size: 15px; color: #cdd6f4; line-height: 1.6; }
    .jp-corpus-mark { background: rgba(203,166,247,0.18); color: #cba6f7; border-radius: 2px; padding: 0 1px; }
    .jp-corpus-en { font-size: 13px; color: #6c7086; line-height: 1.5; }
    .jp-corpus-empty { font-size: 13px; color: #6c7086; padding: 8px 0; }
</style>
