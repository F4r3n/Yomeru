<script lang="ts">
    import type { SrsCard, KanjiEntry, ExampleEntry } from "../shared/types.ts";

    let dueCards = $state<SrsCard[]>([]);
    let currentIdx = $state(0);
    let showBack = $state(false);
    let nextDueMsg = $state("");
    let stagingCount = $state(0);
    let graduatedMsg = $state("");
    let kanjiEntries = $state<KanjiEntry[]>([]);
    let corpusExamples = $state<ExampleEntry[]>([]);
    let activeCardTab = $state<"word" | "kanji" | "examples">("word");

    let currentCard = $derived(dueCards[currentIdx] ?? null);
    let reviewDone = $derived(!currentCard);
    let statsText = $derived(
        `${dueCards.length} card${dueCards.length !== 1 ? "s" : ""} due` +
        (stagingCount > 0 ? ` · ${stagingCount} new` : ""),
    );

    $effect(() => {
        loadReview();
    });

    async function loadReview() {
        const [dueRes, stagingRes] = await Promise.all([
            browser.runtime.sendMessage({ type: "GET_DUE" }),
            browser.runtime.sendMessage({ type: "GET_STAGING" }),
        ]);
        dueCards = (dueRes as { cards: SrsCard[] }).cards ?? [];
        stagingCount = (stagingRes as { cards: SrsCard[] }).cards?.length ?? 0;
        currentIdx = 0;
        showBack = false;
        kanjiEntries = [];
        nextDueMsg = "";
    }

    async function revealAnswer() {
        showBack = true;
        activeCardTab = "word";
        kanjiEntries = [];
        corpusExamples = [];
        if (!currentCard) return;
        const [kanjiRes, exRes] = await Promise.all([
            browser.runtime.sendMessage({ type: "GET_KANJI", payload: { word: currentCard.word } }),
            browser.runtime.sendMessage({ type: "GET_EXAMPLES", payload: { word: currentCard.word } }),
        ]);
        kanjiEntries = (kanjiRes as { entries: KanjiEntry[] }).entries ?? [];
        corpusExamples = (exRes as { entries: ExampleEntry[] }).entries ?? [];
    }

    async function rate(rating: number) {
        if (!currentCard) return;
        const card = currentCard;
        const res = await browser.runtime.sendMessage({
            type: "REVIEW_CARD",
            payload: { word: card.word, rating },
        }) as { success?: boolean; graduated?: boolean };
        if (res.graduated) {
            graduatedMsg = `「${card.word}」 graduated — removed from review queue.`;
            setTimeout(() => { graduatedMsg = ""; }, 4000);
        } else if (rating <= 2) {
            dueCards = [...dueCards, card];
        }
        currentIdx++;
        showBack = false;
        activeCardTab = "word";
        kanjiEntries = [];
        corpusExamples = [];
        if (reviewDone) await computeNextDue();
    }

    async function computeNextDue() {
        const res = await browser.runtime.sendMessage({ type: "GET_ALL_CARDS" });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        const now = Date.now();
        const next = cards.reduce(
            (m, c) => (c.due_ms > now && c.due_ms < m ? c.due_ms : m),
            Infinity,
        );
        if (next < Infinity) {
            const mins = Math.round((next - now) / 60_000);
            nextDueMsg = mins < 60
                ? `Next card due in ${mins} min`
                : `Next card due in ${Math.round(mins / 60)} hr`;
        }
    }
</script>

<div class="review-stats">{statsText}</div>

{#if graduatedMsg}
    <div class="toast-graduated">{graduatedMsg}</div>
{/if}

{#if reviewDone}
    <div class="review-done">
        <p>No cards due right now.</p>
        {#if nextDueMsg}<p class="next-due">{nextDueMsg}</p>{/if}
    </div>
{:else}
    <div class="card">
        <div class="card-front">
            <div class="card-word-wrap">
                <ruby class="card-word-furigana">
                    {currentCard?.word}<rt>{currentCard?.reading}</rt>
                </ruby>
            </div>
        </div>
        {#if showBack}
            <div class="card-tabs">
                <button
                    class="card-tab"
                    class:card-tab--active={activeCardTab === "word"}
                    onclick={() => (activeCardTab = "word")}>Word</button
                >
                {#if kanjiEntries.length > 0}
                    <button
                        class="card-tab"
                        class:card-tab--active={activeCardTab === "kanji"}
                        onclick={() => (activeCardTab = "kanji")}>Kanji</button
                    >
                {/if}
                <button
                    class="card-tab"
                    class:card-tab--active={activeCardTab === "examples"}
                    onclick={() => (activeCardTab = "examples")}>Examples</button
                >
            </div>

            {#if activeCardTab === "word"}
                <div class="card-back">
                    {#if currentCard?.senses?.length}
                        <div class="card-senses">
                            {#each currentCard.senses.slice(0, 3) as sense, si}
                                {@const g = sense.glosses.map((g) => g.text).join("; ")}
                                {#if g}
                                    <div class="card-gloss">
                                        <span class="card-num">{si + 1}.</span>{g}
                                    </div>
                                {/if}
                            {/each}
                        </div>
                    {:else}
                        <div class="card-meaning">{currentCard?.meaning_en}</div>
                    {/if}
                </div>
            {:else if activeCardTab === "kanji"}
                <div class="kanji-breakdown">
                    {#each kanjiEntries as k}
                        <div class="kanji-row">
                            <span class="kanji-char">{k.literal}</span>
                            <div class="kanji-info">
                                {#if k.on_readings.length}
                                    <span class="kanji-on">{k.on_readings.join("、")}</span>
                                {/if}
                                {#if k.kun_readings.length}
                                    <span class="kanji-kun">{k.kun_readings.join("、")}</span>
                                {/if}
                                <span class="kanji-meaning">{k.meanings.slice(0, 3).join(", ")}</span>
                            </div>
                        </div>
                    {/each}
                </div>
            {:else}
                <div class="examples">
                    {#each corpusExamples as ex}
                        <div class="example-row">
                            <div class="example-text">
                                <div>{ex.japanese}</div>
                                <div class="example-en">{ex.english}</div>
                            </div>
                        </div>
                    {/each}
                    {#if corpusExamples.length === 0}
                        <div class="examples-empty">No examples found.</div>
                    {/if}
                </div>
            {/if}
        {/if}
        <div class="card-actions">
            {#if !showBack}
                <button class="btn-show" onclick={revealAnswer}>Show answer</button>
            {:else}
                <div class="rating-buttons">
                    <button class="rating-btn r1" onclick={() => rate(1)}>Again</button>
                    <button class="rating-btn r3" onclick={() => rate(3)}>Hard</button>
                    <button class="rating-btn r4" onclick={() => rate(4)}>Good</button>
                    <button class="rating-btn r5" onclick={() => rate(5)}>Easy</button>
                </div>
            {/if}
        </div>
    </div>
{/if}

<style>
    .review-stats {
        font-size: 13px;
        color: var(--subtext);
        margin-bottom: 12px;
    }

    .toast-graduated {
        background: var(--green);
        color: var(--bg);
        border-radius: 6px;
        font-size: 12px;
        font-weight: 600;
        padding: 6px 12px;
        margin-bottom: 10px;
    }

    .review-done {
        text-align: center;
        padding: 32px;
        color: var(--subtext);
    }
    .next-due {
        margin-top: 8px;
        color: var(--accent);
    }

    .card {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 24px 20px 16px;
        text-align: center;
    }
    .card-word-wrap {
        margin-bottom: 12px;
    }
    .card-word-furigana {
        font-size: 42px;
        font-weight: 700;
        color: var(--blue);
        ruby-align: center;
    }
    .card-word-furigana rt {
        font-size: 16px;
        font-weight: 400;
        color: var(--green);
    }
    .card-tabs {
        display: flex;
        gap: 4px;
        border-bottom: 1px solid var(--border);
        margin: 16px 0 12px;
    }
    .card-tab {
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        color: var(--subtext);
        cursor: pointer;
        font-size: 12px;
        font-family: inherit;
        padding: 4px 12px;
        margin-bottom: -1px;
        transition: color 0.15s;
    }
    .card-tab:hover { color: var(--text); }
    .card-tab--active { color: var(--accent); border-bottom-color: var(--accent); }

    .card-senses {
        text-align: left;
        font-size: 14px;
        color: var(--text);
    }
    .card-gloss {
        margin-bottom: 4px;
    }
    .card-num {
        color: var(--subtext);
        margin-right: 4px;
        font-size: 12px;
    }
    .card-meaning {
        font-size: 16px;
        color: var(--text);
        margin-bottom: 4px;
    }
    .card-actions {
        margin-top: 20px;
    }

    .btn-show {
        background: var(--accent);
        border: none;
        border-radius: 6px;
        color: var(--bg);
        cursor: pointer;
        font-size: 14px;
        font-weight: 600;
        padding: 8px 32px;
        transition: opacity 0.15s;
    }
    .btn-show:hover {
        opacity: 0.85;
    }

    .rating-buttons {
        display: flex;
        gap: 8px;
        justify-content: center;
    }
    .rating-btn {
        border: none;
        border-radius: 6px;
        cursor: pointer;
        font-size: 13px;
        font-weight: 600;
        padding: 7px 16px;
        transition: opacity 0.15s;
    }
    .rating-btn:hover {
        opacity: 0.85;
    }
    .r1 { background: var(--red);    color: var(--bg); }
    .r3 { background: var(--yellow); color: var(--bg); }
    .r4 { background: var(--green);  color: var(--bg); }
    .r5 { background: var(--blue);   color: var(--bg); }

    .examples-empty { font-size: 13px; color: var(--subtext); padding: 4px 0; text-align: left; }

    .kanji-breakdown {
        display: flex;
        flex-direction: column;
        gap: 8px;
        text-align: left;
    }
    .kanji-row {
        display: flex;
        align-items: flex-start;
        gap: 12px;
    }
    .kanji-char {
        font-size: 28px;
        color: var(--blue);
        min-width: 36px;
        text-align: center;
    }
    .kanji-info {
        display: flex;
        flex-direction: column;
        gap: 2px;
        font-size: 12px;
    }
    .kanji-on     { color: var(--yellow);  }
    .kanji-kun    { color: var(--green);   }
    .kanji-meaning { color: var(--subtext); }

    .examples {
        display: flex;
        flex-direction: column;
        gap: 4px;
        text-align: left;
    }
    .example-row {
        display: flex;
        align-items: flex-start;
        gap: 6px;
        color: var(--text);
    }
    .example-text {
        flex: 1;
    }
    .example-text > div:first-child {
        font-size: 16px;
        line-height: 1.6;
    }
    .example-en {
        color: var(--subtext);
        font-size: 13px;
        margin-top: 3px;
    }
</style>
