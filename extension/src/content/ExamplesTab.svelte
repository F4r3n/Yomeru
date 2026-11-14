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

    /** Splits at the word the build marked, else at its first occurrence. */
    function splitAtWord(ex: ExampleEntry, w: string): [string, string, string] | null {
        if (ex.mark) {
            const cps = Array.from(ex.japanese);
            const [from, len] = ex.mark;
            return [
                cps.slice(0, from).join(""),
                cps.slice(from, from + len).join(""),
                cps.slice(from + len).join(""),
            ];
        }
        if (!w) return null;
        const idx = ex.japanese.indexOf(w);
        if (idx === -1) return null;
        return [ex.japanese.slice(0, idx), w, ex.japanese.slice(idx + w.length)];
    }
</script>

{#if examples.length > 0}
    <div class="jp-corpus">
        {#each examples as ex}
            {@const parts = splitAtWord(ex, word)}
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
