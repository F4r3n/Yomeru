<script lang="ts">
    import ReviewTab from "./ReviewTab.svelte";
    import NewWordsTab from "./NewWordsTab.svelte";
    import WordListTab from "./WordListTab.svelte";
    import SettingsTab from "./SettingsTab.svelte";
    import AboutTab from "./AboutTab.svelte";

    type Tab = "review" | "new" | "words" | "settings" | "about";

    let tab = $state<Tab>("review");
    let enabled = $state(true);
    let stagingCount = $state(0);

    $effect(() => {
        browser.storage.local.get("enabled").then((res) => {
            enabled = (res as { enabled?: boolean }).enabled ?? true;
        });
        browser.runtime.sendMessage({ type: "GET_STAGING" })
            .then((res) => {
                stagingCount = (res as { cards: unknown[] }).cards?.length ?? 0;
            })
            .catch(() => {});
    });

    async function toggleEnabled() {
        enabled = !enabled;
        await browser.storage.local.set({ enabled });
    }

    function onStagingChange(n: number) {
        stagingCount = n;
    }
</script>

<header>
    <div class="header-top">
        <h1>Yomeru</h1>
        <button class="toggle-btn" class:enabled onclick={toggleEnabled}>
            {enabled ? "Stop" : "Start"}
        </button>
    </div>
    <nav>
        <button class="tab" class:active={tab === "review"} onclick={() => (tab = "review")}>Review</button>
        <button class="tab" class:active={tab === "new"}    onclick={() => (tab = "new")}>New Words{stagingCount > 0 ? ` (${stagingCount})` : ""}</button>
        <button class="tab" class:active={tab === "words"}  onclick={() => (tab = "words")}>Word List</button>
        <button class="tab" class:active={tab === "settings"} onclick={() => (tab = "settings")}>Settings</button>
        <button class="tab" class:active={tab === "about"}    onclick={() => (tab = "about")}>About</button>
    </nav>
</header>

<main>
    {#if tab === "review"}
        <ReviewTab onstagingchange={onStagingChange} />
    {:else if tab === "new"}
        <NewWordsTab onstagingchange={onStagingChange} />
    {:else if tab === "words"}
        <WordListTab />
    {:else if tab === "settings"}
        <SettingsTab />
    {:else}
        <AboutTab />
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
        width: 440px;
        min-height: 300px;
        padding: 0 0 16px;
    }

    header {
        background: var(--surface);
        padding: 12px 16px 0;
        border-bottom: 1px solid var(--border);
    }
    .header-top {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 10px;
    }
    h1 {
        font-size: 16px;
        font-weight: 700;
        color: var(--blue);
    }
    .toggle-btn {
        border: none;
        border-radius: 6px;
        cursor: pointer;
        font-size: 12px;
        font-weight: 600;
        padding: 4px 14px;
        background: var(--red);
        color: var(--bg);
        transition: opacity 0.15s;
    }
    .toggle-btn.enabled {
        background: var(--green);
    }
    .toggle-btn:hover {
        opacity: 0.85;
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
        transition: color 0.15s, border-color 0.15s;
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
</style>
