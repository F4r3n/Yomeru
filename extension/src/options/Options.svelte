<script lang="ts">
    import type { SrsCard } from "../shared/types.ts";

    type Tab = "review" | "words";

    let tab = $state<Tab>("review");

    // ── Review state ─────────────────────────────────────────────────────────────
    let dueCards = $state<SrsCard[]>([]);
    let currentIdx = $state(0);
    let showBack = $state(false);
    let nextDueMsg = $state("");

    let currentCard = $derived(dueCards[currentIdx] ?? null);
    let reviewDone = $derived(!currentCard);
    let statsText = $derived(
        `${dueCards.length} card${dueCards.length !== 1 ? "s" : ""} due`,
    );

    // ── Words state ───────────────────────────────────────────────────────────────
    let allCards = $state<SrsCard[]>([]);
    let searchQuery = $state("");

    let filteredCards = $derived(
        allCards
            .filter((c) => {
                if (!searchQuery) return true;
                const q = searchQuery.toLowerCase();
                return (
                    c.word.includes(q) ||
                    c.reading.includes(q) ||
                    c.meaning_en.toLowerCase().includes(q)
                );
            })
            .sort((a, b) => a.due_ms - b.due_ms),
    );

    $effect(() => {
        if (tab === "review") loadReview();
        else loadWords();
    });

    async function loadReview() {
        const res = await browser.runtime.sendMessage({ type: "GET_DUE" });
        dueCards = (res as { cards: SrsCard[] }).cards ?? [];
        currentIdx = 0;
        showBack = false;
        nextDueMsg = "";
    }

    async function loadWords() {
        const res = await browser.runtime.sendMessage({
            type: "GET_ALL_CARDS",
        });
        allCards = (res as { cards: SrsCard[] }).cards ?? [];
    }

    async function rate(rating: number) {
        if (!currentCard) return;
        const card = currentCard;
        await browser.runtime.sendMessage({
            type: "REVIEW_CARD",
            payload: { word: card.word, rating },
        });
        if (rating <= 2) {
            dueCards = [...dueCards, card];
        }
        currentIdx++;
        showBack = false;
        if (reviewDone) await computeNextDue();
    }

    async function computeNextDue() {
        const res = await browser.runtime.sendMessage({
            type: "GET_ALL_CARDS",
        });
        const cards = (res as { cards: SrsCard[] }).cards ?? [];
        const now = Date.now();
        const next = cards.reduce(
            (m, c) => (c.due_ms > now && c.due_ms < m ? c.due_ms : m),
            Infinity,
        );
        if (next < Infinity) {
            const mins = Math.round((next - now) / 60_000);
            nextDueMsg =
                mins < 60
                    ? `Next card due in ${mins} min`
                    : `Next card due in ${Math.round(mins / 60)} hr`;
        }
    }

    async function deleteCard(word: string) {
        await browser.runtime.sendMessage({
            type: "DELETE_CARD",
            payload: { word },
        });
        allCards = allCards.filter((c) => c.word !== word);
    }

    function dueLabel(ms: number): string {
        const diff = ms - Date.now();
        if (diff <= 0) return "Due now";
        const h = Math.round(diff / 3_600_000);
        return h < 24 ? `${h}h` : `${Math.round(diff / 86_400_000)}d`;
    }

    function dueClass(ms: number): string {
        const diff = ms - Date.now();
        if (diff <= 0) return "overdue";
        if (diff < 86_400_000) return "today";
        return "future";
    }
</script>

<header>
    <h1>Japanese Reader</h1>
    <nav>
        <button
            class="tab"
            class:active={tab === "review"}
            onclick={() => (tab = "review")}>Review</button
        >
        <button
            class="tab"
            class:active={tab === "words"}
            onclick={() => (tab = "words")}>Word List</button
        >
    </nav>
</header>

<main>
    {#if tab === "review"}
        <div class="review-stats">{statsText}</div>
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
                    <div class="card-back">
                        {#if currentCard?.senses?.length}
                            <div class="card-senses">
                                {#each currentCard.senses.slice(0, 3) as sense, si}
                                    {@const g = sense.glosses
                                        .filter((g) => g.lang === "eng")
                                        .map((g) => g.text)
                                        .join("; ")}
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
                {/if}
                <div class="card-actions">
                    {#if !showBack}
                        <button
                            class="btn-show"
                            onclick={() => (showBack = true)}
                            >Show answer</button
                        >
                    {:else}
                        <div class="rating-buttons">
                            <button
                                class="rating-btn r1"
                                onclick={() => rate(1)}>Again</button
                            >
                            <button
                                class="rating-btn r3"
                                onclick={() => rate(3)}>Hard</button
                            >
                            <button
                                class="rating-btn r4"
                                onclick={() => rate(4)}>Good</button
                            >
                            <button
                                class="rating-btn r5"
                                onclick={() => rate(5)}>Easy</button
                            >
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    {:else}
        <div class="word-list-header">
            <span class="word-count">{filteredCards.length} words</span>
            <input
                class="word-search"
                type="search"
                placeholder="Search…"
                bind:value={searchQuery}
            />
        </div>
        <table class="word-table">
            <thead>
                <tr
                    ><th>Word</th><th>Reading</th><th>Meaning</th><th>Due</th
                    ><th></th></tr
                >
            </thead>
            <tbody>
                {#each filteredCards as card (card.word)}
                    <tr>
                        <td class="td-word">{card.word}</td>
                        <td class="td-reading">{card.reading}</td>
                        <td class="td-meaning">{card.meaning_en}</td>
                        <td class="td-due {dueClass(card.due_ms)}"
                            >{dueLabel(card.due_ms)}</td
                        >
                        <td
                            ><button
                                class="btn-delete"
                                onclick={() => deleteCard(card.word)}
                                >Delete</button
                            ></td
                        >
                    </tr>
                {/each}
            </tbody>
        </table>
    {/if}
</main>

<style>
    :global(:root) {
        --bg: #1e1e2e;
        --surface: #313244;
        --border: #45475a;
        --text: #cdd6f4;
        --subtext: #a6adc8;
        --accent: #cba6f7;
        --green: #a6e3a1;
        --red: #f38ba8;
        --yellow: #f9e2af;
        --blue: #89dceb;
    }
    :global(*) {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }
    :global(body) {
        background: var(--bg);
        color: var(--text);
        font-family: "Noto Sans JP", "Segoe UI", sans-serif;
        font-size: 14px;
        min-width: 440px;
        min-height: 300px;
        padding: 0 0 16px;
    }

    header {
        background: var(--surface);
        padding: 12px 16px 0;
        border-bottom: 1px solid var(--border);
    }
    h1 {
        font-size: 16px;
        font-weight: 700;
        color: var(--blue);
        margin-bottom: 10px;
    }
    nav {
        display: flex;
        gap: 4px;
    }

    .tab {
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        color: var(--subtext);
        cursor: pointer;
        padding: 6px 12px;
        font-size: 13px;
        transition:
            color 0.15s,
            border-color 0.15s;
    }
    .tab.active {
        color: var(--accent);
        border-bottom-color: var(--accent);
    }
    .tab:hover {
        color: var(--text);
    }

    main {
        padding: 16px;
    }

    .review-stats {
        font-size: 13px;
        color: var(--subtext);
        margin-bottom: 12px;
    }

    .card {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 24px 20px 16px;
        text-align: center;
    }
    .card-word {
        font-size: 42px;
        font-weight: 700;
        color: var(--blue);
        margin-bottom: 8px;
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
    .r1 {
        background: var(--red);
        color: var(--bg);
    }
    .r3 {
        background: var(--yellow);
        color: var(--bg);
    }
    .r4 {
        background: var(--green);
        color: var(--bg);
    }
    .r5 {
        background: var(--blue);
        color: var(--bg);
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

    .word-list-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 12px;
    }
    .word-count {
        color: var(--subtext);
        font-size: 13px;
    }
    .word-search {
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text);
        font-size: 13px;
        padding: 4px 10px;
        outline: none;
        width: 180px;
    }
    .word-search:focus {
        border-color: var(--accent);
    }

    .word-table {
        width: 100%;
        border-collapse: collapse;
        font-size: 13px;
    }
    .word-table th {
        text-align: left;
        padding: 6px 8px;
        border-bottom: 1px solid var(--border);
        color: var(--subtext);
        font-weight: 500;
    }
    .word-table td {
        padding: 6px 8px;
        border-bottom: 1px solid var(--border);
        vertical-align: top;
    }
    .word-table tr:last-child td {
        border-bottom: none;
    }

    .td-word {
        font-size: 18px;
        color: var(--blue);
    }
    .td-reading {
        color: var(--green);
    }
    .td-meaning {
        color: var(--text);
        max-width: 180px;
    }
    .td-due.overdue {
        color: var(--red);
    }
    .td-due.today {
        color: var(--yellow);
    }
    .td-due.future {
        color: var(--subtext);
    }

    .btn-delete {
        background: none;
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--red);
        cursor: pointer;
        font-size: 11px;
        padding: 2px 8px;
    }
    .btn-delete:hover {
        background: var(--surface);
    }
</style>
