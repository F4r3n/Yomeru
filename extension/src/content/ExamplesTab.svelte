<script lang="ts">
    import type { ExampleEntry } from "../shared/types.ts";

    let {
        examples,
        fetched,
    }: {
        examples: ExampleEntry[];
        fetched: boolean;
    } = $props();
</script>

{#if examples.length > 0}
    <div class="jp-corpus">
        {#each examples as ex}
            <div class="jp-corpus-ex">
                <div class="jp-corpus-jp">{ex.japanese}</div>
                <div class="jp-corpus-en">{ex.english}</div>
            </div>
        {/each}
    </div>
{:else if fetched}
    <div class="jp-corpus-empty">No examples found.</div>
{/if}
