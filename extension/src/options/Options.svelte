<script lang="ts">
    import ReviewTab from "./ReviewTab.svelte";
    import NewWordsTab from "./NewWordsTab.svelte";
    import WordListTab from "./WordListTab.svelte";
    import LookupTab from "./LookupTab.svelte";
    import SettingsTab from "./SettingsTab.svelte";
    import AboutTab from "./AboutTab.svelte";

    type Tab = "review" | "new" | "words" | "lookup" | "settings" | "about";

    let tab = $state<Tab>("review");
    let enabled = $state(true);
    let stagingCount = $state(0);
    let menuOpen = $state(false);

    function selectTab(t: Tab) {
        tab = t;
        menuOpen = false;
    }

    function onDocClick(e: MouseEvent) {
        if (!menuOpen) return;
        const target = e.target as HTMLElement | null;
        if (target && target.closest(".overflow-wrap")) return;
        menuOpen = false;
    }

    async function loadStagingCount() {
        try {
            const res = await browser.runtime.sendMessage({ type: "GET_STAGING" });
            stagingCount = (res as { cards: unknown[] }).cards?.length ?? 0;
        } catch {}
    }

    $effect(() => {
        browser.storage.local.get("enabled").then((res) => {
            enabled = (res as { enabled?: boolean }).enabled ?? true;
        });
        loadStagingCount();
        let pending: ReturnType<typeof setTimeout> | null = null;
        const handler = (changes: Record<string, browser.storage.StorageChange>, area: string) => {
            if (area !== "local" || !("_yomeru_db_v" in changes)) return;
            if (pending) clearTimeout(pending);
            pending = setTimeout(() => { pending = null; loadStagingCount(); }, 150);
        };
        browser.storage.onChanged.addListener(handler);
        return () => {
            if (pending) clearTimeout(pending);
            browser.storage.onChanged.removeListener(handler);
        };
    });

    async function toggleEnabled() {
        enabled = !enabled;
        await browser.storage.local.set({ enabled });
    }

    function onStagingChange(n: number) {
        stagingCount = n;
    }

    const isPopup = window.innerWidth <= 500;

    if (!isPopup) document.body.classList.add("tab-mode");

    function openInTab() {
        browser.tabs.create({ url: browser.runtime.getURL("options.html") });
        window.close();
    }
</script>

<header>
    <div class="header-top">
        <h1>Yomeru</h1>
        <div class="header-actions">
            {#if isPopup}
                <button class="icon-btn" onclick={openInTab} title="Open in new tab" aria-label="Open in new tab">⊞</button>
            {/if}
            <button class="toggle-btn" class:enabled onclick={toggleEnabled}>
                {enabled ? "Stop" : "Start"}
            </button>
        </div>
    </div>
    <nav>
        <button class="tab" class:active={tab === "review"} onclick={() => selectTab("review")}>Review</button>
        <button class="tab" class:active={tab === "new"}    onclick={() => selectTab("new")}>New Words{stagingCount > 0 ? ` (${stagingCount})` : ""}</button>
        <button class="tab" class:active={tab === "words"}  onclick={() => selectTab("words")}>Word List</button>
        <button class="tab" class:active={tab === "lookup"} onclick={() => selectTab("lookup")}>Lookup</button>
        <div class="overflow-wrap">
            <button
                class="tab overflow"
                class:active={tab === "settings" || tab === "about"}
                aria-label="More"
                aria-haspopup="true"
                aria-expanded={menuOpen}
                onclick={() => (menuOpen = !menuOpen)}>⋯</button>
            {#if menuOpen}
                <div class="overflow-menu" role="menu">
                    <button class="overflow-item" class:active={tab === "settings"} role="menuitem" onclick={() => selectTab("settings")}>Settings</button>
                    <button class="overflow-item" class:active={tab === "about"} role="menuitem" onclick={() => selectTab("about")}>About</button>
                </div>
            {/if}
        </div>
    </nav>
</header>

<svelte:window onclick={onDocClick} />

<main>
    {#if tab === "review"}
        <ReviewTab onstagingchange={onStagingChange} />
    {:else if tab === "new"}
        <NewWordsTab onstagingchange={onStagingChange} />
    {:else if tab === "words"}
        <WordListTab />
    {:else if tab === "lookup"}
        <LookupTab />
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
    :global(body.tab-mode) {
        width: 720px;
        max-width: 100%;
        margin: 0 auto;
        min-height: 100vh;
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
    .header-actions {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .icon-btn {
        background: none;
        border: none;
        border-radius: 6px;
        color: var(--subtext);
        cursor: pointer;
        font-size: 16px;
        line-height: 1;
        padding: 4px 6px;
        transition: color 0.15s;
    }
    .icon-btn:hover {
        color: var(--text);
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
        align-items: stretch;
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
    .overflow-wrap {
        position: relative;
        margin-left: auto;
    }
    .tab.overflow {
        font-size: 16px;
        padding: 6px 10px;
        line-height: 1;
    }
    .overflow-menu {
        position: absolute;
        top: calc(100% + 2px);
        right: 0;
        background: var(--surface);
        border: 1px solid var(--border);
        border-radius: 6px;
        box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
        display: flex;
        flex-direction: column;
        min-width: 120px;
        z-index: 10;
        overflow: hidden;
    }
    .overflow-item {
        background: none;
        border: none;
        color: var(--subtext);
        cursor: pointer;
        font-size: 13px;
        padding: 8px 14px;
        text-align: left;
    }
    .overflow-item:hover {
        background: var(--border);
        color: var(--text);
    }
    .overflow-item.active {
        color: var(--accent);
    }
    main {
        padding: 16px;
    }
</style>
