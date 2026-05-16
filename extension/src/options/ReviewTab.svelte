<script lang="ts">
    import type { SrsCard, KanjiEntry, ExampleEntry, WordEntry } from "../shared/types.ts";
    import { buildEntryMap } from "./dict-lookup.ts";

    let { onstagingchange }: { onstagingchange?: (n: number) => void } = $props();

    let dueCards = $state<SrsCard[]>([]);
    let entriesByWord = $state<Record<string, WordEntry | null>>({});
    let skippedWords = $state<string[]>([]);
    let currentIdx = $state(0);
    let showBack = $state(false);
    let nextDueMsg = $state("");
    let dueCount = $state(0);
    let stagingCount = $state(0);
    let graduatedMsg = $state("");
    let kanjiEntries = $state<KanjiEntry[]>([]);
    let corpusExamples = $state<ExampleEntry[]>([]);
    let examplesLoading = $state(false);
    let activeCardTab = $state<"word" | "kanji" | "examples">("word");
    let reviewStarted = $state(false);
    let isRating = $state(false);
    let reviewError = $state("");

    // Card ids reviewed in the current session — excluded from the next
    // loadReview so they don't reappear immediately after Done.
    let sessionReviewed = new Set<string>();

    let currentCard = $derived(dueCards[currentIdx] ?? null);
    let currentEntry = $derived(currentCard ? entriesByWord[currentCard.word] ?? null : null);
    let reviewDone = $derived(reviewStarted && !currentCard);
    let progressText = $derived(`${currentIdx + 1} / ${dueCards.length}`);
    let recallGlosses = $derived.by(() => {
        if (!currentEntry) return [] as string[];
        const out: string[] = [];
        for (const s of currentEntry.senses.slice(0, 3)) {
            const g = s.glosses.map((x) => x.text).join("; ");
            if (g) out.push(g);
        }
        return out;
    });
    let statsText = $derived(
        `${dueCount} card${dueCount !== 1 ? "s" : ""} due` +
        (stagingCount > 0 ? ` · ${stagingCount} new` : ""),
    );

    async function attachEntries(cards: SrsCard[]): Promise<{ kept: SrsCard[]; skipped: string[]; entries: Record<string, WordEntry | null> }> {
        const entries = await buildEntryMap(cards.map((c) => c.word));
        const kept: SrsCard[] = [];
        const skipped: string[] = [];
        for (const c of cards) {
            if (entries[c.word]) kept.push(c);
            else skipped.push(c.word);
        }
        return { kept, skipped, entries };
    }

    $effect(() => {
        loadReview();
    });

    async function loadReview() {
        try {
            const [dueRes, stagingRes] = await Promise.all([
                browser.runtime.sendMessage({ type: "GET_DUE" }),
                browser.runtime.sendMessage({ type: "GET_STAGING" }),
            ]);
            const excluded = sessionReviewed;
            sessionReviewed = new Set();
            const all = (dueRes as { cards: SrsCard[] }).cards ?? [];
            const filtered = excluded.size > 0 ? all.filter(c => !excluded.has(c.id)) : all;
            const { kept, skipped, entries } = await attachEntries(filtered);
            entriesByWord = entries;
            skippedWords = skipped;
            dueCards = kept;
            dueCount = dueCards.length;
            stagingCount = (stagingRes as { cards: SrsCard[] }).cards?.length ?? 0;
            onstagingchange?.(stagingCount);
            currentIdx = 0;
            showBack = false;
            kanjiEntries = [];
            nextDueMsg = "";
            reviewStarted = false;
        } catch (e) {
            console.error("[yomeru] loadReview failed:", e);
        }
    }

    async function promoteAndReview() {
        sessionReviewed = new Set();
        const res = await browser.runtime.sendMessage({ type: "PROMOTE_BATCH" });
        const { cards, stagingCount: remaining } = res as { cards: SrsCard[]; stagingCount: number };
        const { kept, skipped, entries } = await attachEntries(cards);
        entriesByWord = entries;
        skippedWords = skipped;
        dueCards = kept;
        dueCount = dueCards.length;
        stagingCount = remaining;
        onstagingchange?.(remaining);
        currentIdx = 0;
        showBack = false;
        kanjiEntries = [];
        reviewStarted = true;
    }

    function startReview() {
        sessionReviewed = new Set();
        reviewStarted = true;
    }

    async function revealAnswer() {
        showBack = true;
        activeCardTab = "word";
        kanjiEntries = [];
        corpusExamples = [];
        if (!currentCard) return;
        examplesLoading = true;
        try {
            const [kanjiRes, exRes] = await Promise.all([
                browser.runtime.sendMessage({ type: "GET_KANJI", payload: { word: currentCard.word } }),
                browser.runtime.sendMessage({ type: "GET_EXAMPLES", payload: { word: currentCard.word } }),
            ]);
            kanjiEntries = (kanjiRes as { entries: KanjiEntry[] }).entries ?? [];
            corpusExamples = (exRes as { entries: ExampleEntry[] }).entries ?? [];
        } finally {
            examplesLoading = false;
        }
    }

    async function rate(rating: number) {
        if (!currentCard || isRating) return;
        isRating = true;
        const card = currentCard;
        try {
            const res = await browser.runtime.sendMessage({
                type: "REVIEW_CARD",
                payload: { word: card.word, direction: card.direction, rating },
            }) as { success?: boolean; graduated?: boolean; error?: string };
            if (res.error) {
                console.error("[yomeru] REVIEW_CARD failed:", res.error, "word:", card.word);
                reviewError = `Failed to save review for 「${card.word}」`;
                setTimeout(() => { reviewError = ""; }, 5000);
            } else {
                sessionReviewed.add(card.id);
                if (res.graduated) {
                    graduatedMsg = `「${card.word}」 (${card.direction}) graduated — removed from review queue.`;
                    setTimeout(() => { graduatedMsg = ""; }, 4000);
                }
            }
        } catch (e) {
            console.error("[yomeru] REVIEW_CARD threw:", e, "word:", card.word);
            reviewError = `Failed to save review for 「${card.word}」`;
            setTimeout(() => { reviewError = ""; }, 5000);
        } finally {
            currentIdx++;
            showBack = false;
            activeCardTab = "word";
            kanjiEntries = [];
            corpusExamples = [];
            isRating = false;
        }
        if (reviewDone) await computeNextDue();
    }

    async function computeNextDue() {
        const res = await browser.runtime.sendMessage({ type: "GET_ALL_CARDS" });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        const now = Date.now();
        const next = cards
            .filter((c) => c.status === "active")
            .reduce(
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
{#if reviewError}
    <div class="toast-error">{reviewError}</div>
{/if}
{#if skippedWords.length > 0}
    <div class="toast-warning">
        Skipped {skippedWords.length} card{skippedWords.length !== 1 ? "s" : ""} — no longer in the dictionary:
        <span class="skipped-list">{skippedWords.join("、")}</span>
    </div>
{/if}

{#if !reviewStarted}
    <div class="review-idle">
        {#if dueCount > 0}
            <p>{dueCount} card{dueCount !== 1 ? "s" : ""} ready for review.</p>
            <button class="btn-start" onclick={startReview}>Start Review</button>
        {:else if stagingCount > 0}
            <p>{stagingCount} new word{stagingCount !== 1 ? "s" : ""} ready to learn.</p>
            <button class="btn-start" onclick={promoteAndReview}>Add new words</button>
        {:else}
            <p>No cards due right now.</p>
            {#if nextDueMsg}<p class="next-due">{nextDueMsg}</p>{/if}
        {/if}
    </div>
{:else if reviewDone}
    <div class="review-done">
        <p>Review complete!</p>
        {#if nextDueMsg}<p class="next-due">{nextDueMsg}</p>{/if}
        <button class="btn-start" onclick={loadReview}>Done</button>
    </div>
{:else}
    <div class="card">
        <div class="card-progress">{progressText}</div>
        <div class="card-direction-badge" class:badge-recall={currentCard?.direction === "recall"}>
            {currentCard?.direction === "recall" ? "Recall" : "Recognition"}
        </div>
        <div class="card-front">
            {#if !showBack && currentCard?.direction === "recall"}
                <div class="card-recall-glosses">
                    {#if recallGlosses.length > 0}
                        {#each recallGlosses as g, gi}
                            <div class="card-recall-gloss">
                                <span class="card-num">{gi + 1}.</span>{g}
                            </div>
                        {/each}
                    {:else}
                        <div class="card-recall-empty">No definition available.</div>
                    {/if}
                </div>
            {:else}
                <div class="card-word-wrap">
                    <ruby class="card-word-furigana">
                        {currentCard?.word}<rt>{currentEntry?.reading_forms[0]?.text ?? ""}</rt>
                    </ruby>
                </div>
            {/if}
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

            <div class="card-tab-content">
                {#if activeCardTab === "word"}
                    <div class="card-back">
                        {#if currentEntry?.senses?.length}
                            <div class="card-senses">
                                {#each currentEntry.senses.slice(0, 3) as sense, si}
                                    {@const g = sense.glosses.map((g) => g.text).join("; ")}
                                    {#if g}
                                        {#if sense.pos?.length}
                                            <div class="card-pos-row">
                                                {#each sense.pos as pos}<span class="card-pos">{pos}</span>{/each}
                                            </div>
                                        {/if}
                                        <div class="card-gloss">
                                            <span class="card-num">{si + 1}.</span>{g}
                                        </div>
                                    {/if}
                                {/each}
                            </div>
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
                    {#if corpusExamples.length > 0}
                        <div class="review-examples">
                            {#each corpusExamples as ex}
                                {@const word = currentCard?.word ?? ""}
                                {@const idx = word ? ex.japanese.indexOf(word) : -1}
                                <div class="review-ex">
                                    <div class="review-ex-jp">
                                        {#if idx !== -1}
                                            {ex.japanese.slice(0, idx)}<mark class="review-ex-mark">{word}</mark>{ex.japanese.slice(idx + word.length)}
                                        {:else}
                                            {ex.japanese}
                                        {/if}
                                    </div>
                                    <div class="review-ex-en">{ex.english}</div>
                                </div>
                            {/each}
                        </div>
                    {:else if showBack && !examplesLoading}
                        <div class="review-examples-empty">No examples found.</div>
                    {/if}
                {/if}
            </div>
        {/if}
        <div class="card-actions">
            {#if !showBack}
                <button class="btn-show" onclick={revealAnswer}>Show answer</button>
            {:else}
                <div class="rating-buttons">
                    <button class="rating-btn r1" disabled={isRating} onclick={() => rate(1)}>Again</button>
                    <button class="rating-btn r3" disabled={isRating} onclick={() => rate(3)}>Hard</button>
                    <button class="rating-btn r4" disabled={isRating} onclick={() => rate(4)}>Good</button>
                    <button class="rating-btn r5" disabled={isRating} onclick={() => rate(5)}>Easy</button>
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

    .toast-error {
        background: var(--red);
        color: var(--bg);
        border-radius: 6px;
        font-size: 12px;
        font-weight: 600;
        padding: 6px 12px;
        margin-bottom: 10px;
    }

    .toast-warning {
        background: var(--yellow, #f9e2af);
        color: var(--bg);
        border-radius: 6px;
        font-size: 12px;
        padding: 6px 12px;
        margin-bottom: 10px;
    }
    .toast-warning .skipped-list {
        font-weight: 600;
        margin-left: 4px;
    }

    .review-idle,
    .review-done {
        text-align: center;
        padding: 32px;
        color: var(--subtext);
    }
    .next-due {
        margin-top: 8px;
        color: var(--accent);
    }

    .btn-start {
        margin-top: 16px;
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
    .btn-start:hover { opacity: 0.85; }

    .card {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 24px 20px 16px;
        text-align: center;
        position: relative;
    }
    .card-progress {
        position: absolute;
        top: 6px;
        right: 10px;
        font-size: 10px;
        color: var(--subtext);
        opacity: 0.55;
        pointer-events: none;
    }
    .card-direction-badge {
        position: absolute;
        top: 6px;
        left: 10px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
        color: var(--green);
        opacity: 0.7;
    }
    .card-direction-badge.badge-recall {
        color: var(--accent);
    }
    .card-recall-glosses {
        text-align: left;
        font-size: 18px;
        color: var(--text);
        padding: 8px 4px;
        margin-bottom: 12px;
    }
    .card-recall-gloss {
        margin-bottom: 6px;
        line-height: 1.4;
    }
    .card-recall-empty {
        color: var(--subtext);
        font-style: italic;
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
    .card-tab-content {
        height: 160px;
        overflow-y: auto;
        scrollbar-gutter: stable;
        text-align: left;
    }
    .review-examples {
        padding: 2px 0;
    }
    .review-ex {
        margin-bottom: 10px;
    }
    .review-ex:last-child {
        margin-bottom: 0;
    }
    .review-ex-jp {
        font-size: 15px;
        color: var(--text);
        line-height: 1.6;
        overflow-wrap: break-word;
        word-break: break-all;
    }
    .review-ex-mark {
        background: rgba(203, 166, 247, 0.18);
        color: var(--accent);
        border-radius: 2px;
        padding: 0 1px;
    }
    .review-ex-en {
        font-size: 13px;
        color: var(--subtext);
        line-height: 1.5;
        overflow-wrap: break-word;
    }
    .review-examples-empty {
        font-size: 13px;
        color: var(--subtext);
        padding: 4px 0;
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

    .card-pos-row {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-bottom: 2px;
        margin-top: 6px;
    }
    .card-pos-row:first-child { margin-top: 0; }
    .card-pos {
        font-size: 10px;
        color: var(--subtext);
        background: var(--surface2, var(--border));
        border-radius: 3px;
        padding: 1px 5px;
    }

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


</style>
