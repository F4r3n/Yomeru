<script lang="ts">
    import type { KanjiEntry } from "../shared/types.ts";

    let { kanjiEntries }: { kanjiEntries: KanjiEntry[] } = $props();

    function jlptLabel(jlpt: number | null): string | null {
        return jlpt != null ? `N${5 - jlpt}` : null;
    }
</script>

<div class="jp-kanji-list">
    {#each kanjiEntries as k (k.literal)}
        <div class="jp-kanji-entry">
            <div class="jp-kanji-header">
                <span class="jp-kanji-char">{k.literal}</span>
                <span class="jp-kanji-meta">
                    {#if k.stroke_count}
                        <span class="jp-kanji-strokes">{k.stroke_count} strokes</span>
                    {/if}
                    {#if jlptLabel(k.jlpt)}
                        <span class="jp-kanji-jlpt">{jlptLabel(k.jlpt)}</span>
                    {/if}
                </span>
            </div>
            {#if k.on_readings.length > 0}
                <div class="jp-kanji-readings">
                    <span class="jp-kanji-rdlabel">On:</span>{k.on_readings.join("、")}
                </div>
            {/if}
            {#if k.kun_readings.length > 0}
                <div class="jp-kanji-readings">
                    <span class="jp-kanji-rdlabel">Kun:</span>{k.kun_readings.join("、")}
                </div>
            {/if}
            {#if k.meanings.length > 0}
                <div class="jp-kanji-meanings">{k.meanings.join("; ")}</div>
            {/if}
        </div>
    {/each}
</div>
