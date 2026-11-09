//! Global CSS — Catppuccin Mocha (dark, default) and Latte (light, applied via
//! `data-theme="light"` on <html>). Three-tier surface contrast: app bg (crust)
//! < rails/header (base) < cards (surface0). Sidebar app-shell layout that
//! collapses to a top tab bar below 800px.

use dioxus::prelude::*;

// Self-hosted Noto Sans JP (japanese subset, from fontsource) so CJK glyphs
// render identically across OSes instead of falling through to whatever the
// browser's default CJK fallback is. unicode-range in `global_css` confines
// Noto to CJK; Latin text still resolves to Segoe UI / system-ui.
const NOTO_400: Asset = asset!("/assets/fonts/NotoSansJP-400.woff2");
const NOTO_500: Asset = asset!("/assets/fonts/NotoSansJP-500.woff2");
const NOTO_700: Asset = asset!("/assets/fonts/NotoSansJP-700.woff2");

const CJK_UNICODE_RANGE: &str = "U+3000-303F, U+3041-3096, U+309B-309F, U+30A0-30FF, \
    U+31F0-31FF, U+3220-3247, U+3280-32CB, U+FF01-FF5E, U+FF61-FF9F, U+FFE0-FFEE, \
    U+4E00-9FFF, U+3400-4DBF, U+F900-FAFF, U+FE30-FE4F";

pub fn global_css() -> String {
    let font_face = format!(
        r#"
@font-face {{
    font-family: 'Noto Sans JP';
    font-style: normal;
    font-weight: 400;
    font-display: swap;
    src: url("{r}") format("woff2");
    unicode-range: {range};
}}
@font-face {{
    font-family: 'Noto Sans JP';
    font-style: normal;
    font-weight: 500;
    font-display: swap;
    src: url("{m}") format("woff2");
    unicode-range: {range};
}}
@font-face {{
    font-family: 'Noto Sans JP';
    font-style: normal;
    font-weight: 700;
    font-display: swap;
    src: url("{b}") format("woff2");
    unicode-range: {range};
}}
"#,
        r = NOTO_400,
        m = NOTO_500,
        b = NOTO_700,
        range = CJK_UNICODE_RANGE,
    );
    format!("{font_face}{BASE_CSS}")
}

const BASE_CSS: &str = r#"
:root {
    /* Catppuccin Mocha (dark) */
    --bg:       #181825; /* crust  — app background */
    --surface:  #1e1e2e; /* base   — sidebar, top bar */
    --card:     #313244; /* surface0 — cards, inputs */
    --border:   #45475a;
    --text:     #cdd6f4;
    --subtext:  #a6adc8;
    --accent:   #cba6f7;
    --green:    #a6e3a1;
    --red:      #f38ba8;
    --yellow:   #f9e2af;
    --blue:     #89dceb;
    --on-accent:#1e1e2e;
}
:root[data-theme="light"] {
    /* Catppuccin Latte (light) */
    --bg:       #eff1f5; /* base   */
    --surface:  #e6e9ef; /* mantle */
    --card:     #ffffff;
    --border:   #bcc0cc;
    --text:     #4c4f69;
    --subtext:  #6c6f85;
    --accent:   #8839ef;
    --green:    #40a02b;
    --red:      #d20f39;
    --yellow:   #df8e1d;
    --blue:     #04a5e5;
    --on-accent:#ffffff;
}

* { box-sizing: border-box; margin: 0; padding: 0; -webkit-tap-highlight-color: transparent; }
html, body { height: 100%; }
html { -webkit-text-size-adjust: 100%; }
body {
    background: var(--bg);
    color: var(--text);
    font-family: "Noto Sans JP", "Segoe UI", system-ui, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
    overscroll-behavior-y: contain;
}
button, a, input, select, textarea { touch-action: manipulation; }

/* ── Layout shell ─────────────────────────────────────────────────── */
.app-shell {
    min-height: 100vh;
    display: grid;
    grid-template-rows: 56px 1fr;
    grid-template-columns: 220px 1fr;
    grid-template-areas:
        "topbar  topbar"
        "sidebar main";
}
.topbar {
    grid-area: topbar;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 20px;
    position: sticky;
    top: 0;
    z-index: 10;
}
.brand {
    display: flex;
    align-items: baseline;
    gap: 10px;
    color: var(--text);
    text-decoration: none;
}
.brand .mark { font-size: 22px; color: var(--accent); }
.brand .name { font-size: 17px; font-weight: 700; letter-spacing: 0.3px; }
.brand .tag  { font-size: 12px; color: var(--subtext); }

.topbar-actions { display: flex; align-items: center; gap: 8px; }
.icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    color: var(--subtext);
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 14px;
    cursor: pointer;
}
.icon-btn:hover { color: var(--text); background: var(--card); }
.icon-btn.lookups-on { color: var(--accent); }
.icon-btn.sync-warning, .icon-btn.sync-warning:hover { color: var(--yellow); }
.icon-btn.sync-spinner { cursor: default; color: var(--subtext); }

.sidebar {
    grid-area: sidebar;
    background: var(--surface);
    border-right: 1px solid var(--border);
    padding: 16px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
}
.sidebar .nav-tab {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border-radius: 6px;
    color: var(--subtext);
    text-decoration: none;
    font-size: 13px;
    font-weight: 500;
    border-left: 2px solid transparent;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
}
.sidebar .nav-tab:hover { color: var(--text); background: var(--card); }
.sidebar .nav-tab.active,
.sidebar .nav-tab.active:hover {
    color: var(--on-accent);
    background: var(--accent);
    border-left-color: var(--accent);
}
.sidebar .nav-tab.active .nav-icon { opacity: 1; }
.sidebar .nav-tab .nav-icon { width: 18px; display: inline-flex; justify-content: center; opacity: 0.85; }

.content {
    grid-area: main;
    padding: 28px 32px 48px;
    overflow-x: hidden;
}
/* `clip` cuts the same overflow without making .content a scroll box, so
   `position: sticky` inside it follows the page scroll (kanji finder). */
@supports (overflow: clip) { .content { overflow-x: clip; } }
.content-inner { max-width: 880px; }

/* ── Responsive: collapse to top tab bar on narrow screens ────────── */
@media (max-width: 800px) {
    .app-shell {
        grid-template-rows: 56px auto 1fr;
        grid-template-columns: 1fr;
        grid-template-areas:
            "topbar"
            "sidebar"
            "main";
    }
    .sidebar {
        flex-direction: row;
        flex-wrap: nowrap;
        overflow-x: auto;
        scrollbar-width: none;
        border-right: none;
        border-bottom: 1px solid var(--border);
        padding: 6px 10px;
        gap: 2px;
    }
    .sidebar::-webkit-scrollbar { display: none; }
    .sidebar .nav-tab {
        border-left: none;
        border-bottom: 2px solid transparent;
        border-radius: 4px;
        padding: 10px 12px;
        white-space: nowrap;
        flex-shrink: 0;
    }
    .sidebar .nav-tab .nav-icon { display: none; }
    .sidebar .nav-tab.active { border-left: none; }
    .content { padding: 16px 14px 32px; }
    .content-inner { max-width: 100%; }
    .brand .tag { display: none; }
    .topbar { padding: 0 14px; }
}

/* ── Phone breakpoint: bigger touch targets, single-column forms ──── */
@media (max-width: 540px) {
    body { font-size: 15px; }
    button {
        padding: 10px 14px;
        font-size: 14px;
        min-height: 40px;
    }
    .icon-btn { min-height: 40px; min-width: 40px; padding: 8px 10px; }
    input, select, textarea {
        font-size: 16px;   /* iOS/Chrome: prevent zoom-on-focus */
        padding: 10px 12px;
        min-height: 44px;
    }
    .page-header {
        flex-direction: column;
        align-items: flex-start;
        gap: 8px;
    }
    .page-header h2 { font-size: 20px; }
    /* All stat cards on one row: three in a two-column wrap left one
       stranded on its own line. */
    .content .stat-grid { grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); grid-template-columns: none; gap: 8px; }
    .stat-card { padding: 10px 12px; min-width: 0; }
    .stat-card .stat-value { font-size: 22px; }
    .stat-card .stat-label { font-size: 10px; letter-spacing: 0.04em; }
    .card { padding: 14px 14px; border-radius: 8px; }
    .review-card { padding: 14px 14px; }
    .review-card .face { padding: 24px 8px; }
    .review-card .face .word { font-size: 30px; }
    .rate-grid { gap: 6px; }
    .rate-grid button { padding: 14px 4px; font-size: 13px; }
    .hero-search input[type="search"] {
        padding: 12px 14px;
    }
    .empty-state { padding: 40px 12px; }
    .empty-state .glyph { font-size: 36px; }
    .headword { font-size: 21px; }
    .toolbar { gap: 8px; }
    .toolbar .count { width: 100%; }
}

/* ── New Words: staged word cards ─────────────────────────────────── */
.staged-item {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: "head actions" "detail detail";
    align-items: center;
    column-gap: 12px;
    cursor: pointer;
}
.si-head { grid-area: head; display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; min-width: 0; }
.si-actions { grid-area: actions; display: flex; gap: 8px; }
.staged-item .entry-detail { grid-area: detail; }

/* Small screens: Accept/Reject only on the tapped word, under its meaning. */
@media (max-width: 700px) {
    .staged-item { grid-template-columns: 1fr; grid-template-areas: "head" "detail" "actions"; }
    .si-actions { display: none; padding-top: 10px; }
    .staged-item.open .si-actions { display: flex; }
    .si-actions button { flex: 1; min-height: 40px; }
}

/* ── Word list: one row per word, its directions side by side ─────── */
.card.word-list { padding: 0; }
.word-item {
    display: grid;
    /* Fixed middle column so directions line up from row to row. */
    grid-template-columns: minmax(120px, 1fr) 270px auto;
    align-items: center;
    gap: 6px 16px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
}
.word-item:last-child { border-bottom: none; }
.wi-word { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
.wi-head { font-size: 16px; color: var(--text); }
.wi-reading { font-size: 12px; color: var(--subtext); }
.wi-dirs { display: flex; flex-direction: column; gap: 4px; }
.wi-dir { display: grid; grid-template-columns: 84px 1fr auto; align-items: center; gap: 8px; font-size: 12px; }
.wi-dir-name { color: var(--subtext); }
.wi-state { color: var(--subtext); opacity: 0.8; }
.wi-actions { display: flex; gap: 6px; justify-content: flex-end; }

/* Phone: word on top, directions below; Reset/Delete only for the row
   that was tapped, so a stray tap can't hit Delete. */
@media (max-width: 700px) {
    .word-item { grid-template-columns: 1fr; padding: 10px 14px; cursor: pointer; }
    .wi-actions { display: none; padding-top: 4px; }
    .word-item.open { background: var(--card); }
    .word-item.open .wi-actions { display: flex; }
    .wi-actions button { flex: 1 1 auto; min-height: 40px; padding-inline: 10px; white-space: nowrap; }
}


/* ── Privacy policy ───────────────────────────────────────────────── */
.privacy h4 { font-size: 15px; margin: 22px 0 6px; color: var(--text); }
.privacy p, .privacy li { font-size: 14px; line-height: 1.6; }
.privacy p { margin: 0 0 8px; }
.privacy ul { margin: 0 0 8px; padding-left: 20px; }
.privacy li { margin-bottom: 4px; }
.privacy code { font-size: 13px; overflow-wrap: anywhere; }

/* ── Safe areas (notches, gesture bars) ───────────────────────────── */
@supports (padding: max(0px)) {
    .topbar  { padding-left:  max(20px, env(safe-area-inset-left));
               padding-right: max(20px, env(safe-area-inset-right)); }
    .content { padding-left:  max(14px, env(safe-area-inset-left));
               padding-right: max(14px, env(safe-area-inset-right));
               padding-bottom: max(32px, env(safe-area-inset-bottom)); }
}

/* ── Icons ───────────────────────────────────────────────────────── */
.icon { display: block; flex-shrink: 0; }
/* Large glyphs read heavy at the default stroke; thin them out. */
.empty-state .glyph .icon { stroke-width: 1.5; }
.spin { display: inline-flex; animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .spin { animation: none; } }

/* ── Inputs & buttons ─────────────────────────────────────────────── */
button {
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
    padding: 7px 14px;
    background: var(--card);
    color: var(--text);
    transition: opacity 0.15s, background 0.15s, transform 0.04s;
}
button:hover:not(:disabled) { opacity: 0.88; }
button:active:not(:disabled) { transform: translateY(1px); }
button:disabled { opacity: 0.4; cursor: not-allowed; }
button.primary { background: var(--accent); color: var(--on-accent); }
button.danger  { background: var(--red);    color: var(--on-accent); }
button.success { background: var(--green);  color: var(--on-accent); }
button.warning { background: var(--yellow); color: var(--on-accent); }
/* Secondary call to action: visible on a card without competing with the
   filled .primary button on the same screen. */
button.outline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
}
button.link {
    background: transparent;
    color: var(--accent);
    padding: 4px 8px;
}
button.link:hover:not(:disabled) { text-decoration: underline; }
button.outline:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); opacity: 1; }

input[type="range"] { accent-color: var(--accent); }

input, select, textarea {
    background: var(--card);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 14px;
    font-family: inherit;
    width: 100%;
}
input:focus, select:focus, textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
}

/* ── Cards & typography ───────────────────────────────────────────── */
.card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px 18px;
    margin-bottom: 12px;
}

.row { display: flex; gap: 8px; align-items: center; }
.col { display: flex; flex-direction: column; gap: 8px; }
.muted { color: var(--subtext); }
.error { color: var(--red); }
.ok    { color: var(--green); }

.headword { font-size: 24px; font-weight: 600; color: var(--text); letter-spacing: 0.2px; }
.kanji-sub { color: var(--subtext); font-size: 16px; font-weight: 500; }
.freq-badge { font-size: 10px; color: var(--subtext); border: 1px solid var(--border);
              border-radius: 999px; padding: 1px 7px; font-weight: 500; align-self: center; white-space: nowrap; }
.reading  { color: var(--subtext); font-size: 14px; margin-top: 2px; }
.pos      { color: var(--yellow);  font-size: 12px; margin-top: 6px; font-style: italic; }
.gloss    { color: var(--text);    font-size: 13px; margin-top: 2px; }

table { width: 100%; border-collapse: collapse; }
th, td { padding: 9px 8px; text-align: left; border-bottom: 1px solid var(--border); font-size: 13px; }
th { color: var(--subtext); font-weight: 500; }

.badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 500;
    background: var(--border);
    color: var(--text);
}
.badge.staging { background: var(--yellow); color: var(--on-accent); }
.badge.active  { background: var(--green);  color: var(--on-accent); }
.badge.due     { background: var(--red);    color: var(--on-accent); }
.badge.new     { background: var(--blue);   color: var(--on-accent); }
.badge.fail    { background: var(--red);    color: var(--on-accent); }

/* ── Word rows (trouble words, session recap) ────────────────────── */
.word-rows { display: flex; flex-direction: column; }
.word-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--border);
    min-width: 0;
}
.word-row:first-child { border-top: none; }
.word-row .w { font-size: 18px; font-weight: 600; white-space: nowrap; }
.word-row .r { color: var(--subtext); font-size: 13px; white-space: nowrap; }
.word-row .g {
    color: var(--subtext);
    font-size: 13px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.word-row .n { color: var(--subtext); font-size: 11px; white-space: nowrap; flex-shrink: 0; }
.word-row .badge { flex-shrink: 0; }
details.info { margin-bottom: 8px; font-size: 12px; color: var(--subtext); }
details.info summary {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
    width: fit-content;
    list-style: none;
    user-select: none;
}
details.info summary::-webkit-details-marker { display: none; }
details.info summary:hover { color: var(--text); }
details.info ul { margin: 6px 0 4px 18px; display: flex; flex-direction: column; gap: 2px; }
.recap-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 12px;
    margin-top: 16px;
}
.recap-grid .card { margin-bottom: 0; }

.loading { color: var(--subtext); padding: 32px; text-align: center; }
.empty   { color: var(--subtext); padding: 32px; text-align: center; }

/* ── Page header ─────────────────────────────────────────────────── */
.page-header {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
}
.page-header h2 {
    font-size: 22px;
    font-weight: 700;
    color: var(--text);
    letter-spacing: 0.2px;
}
.page-header .subtitle { color: var(--subtext); font-size: 13px; margin-top: 2px; }
.page-header .actions { display: flex; gap: 8px; align-items: center; }

.section-title {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--subtext);
    margin-bottom: 8px;
}

/* ── Stats / chips / toolbar ─────────────────────────────────────── */
.stat-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 12px;
    margin-bottom: 16px;
}
.stat-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 14px 16px;
}
.stat-card .stat-value {
    font-size: 26px;
    font-weight: 700;
    color: var(--text);
    line-height: 1.1;
}
.stat-card .stat-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--subtext);
    margin-top: 4px;
}
.stat-card.accent .stat-value { color: var(--accent); }
.stat-card.warn   .stat-value { color: var(--yellow); }
.stat-card.danger .stat-value { color: var(--red); }

/* ── Forecast chart ──────────────────────────────────────────────── */
.forecast {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 16px;
    margin-bottom: 16px;
}
.forecast-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
}
.forecast-head .section-title { margin-bottom: 0; }
.forecast-tabs, .seg-tabs { display: flex; flex-wrap: wrap; gap: 4px; }
.forecast-tabs button, .seg-tabs button {
    font-size: 12px;
    font-weight: 500;
    padding: 4px 11px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--subtext);
    cursor: pointer;
}
.forecast-tabs button:hover, .seg-tabs button:hover { color: var(--text); }
.forecast-tabs button.active, .seg-tabs button.active {
    background: var(--accent);
    color: var(--on-accent);
    border-color: var(--accent);
}
.forecast-chart {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 130px;
}
.forecast-bar-wrap {
    position: relative;
    flex: 1;
    height: 100%;
    display: flex;
    align-items: flex-end;
}
.forecast-bar {
    width: 100%;
    background: var(--accent);
    border-radius: 3px 3px 0 0;
    min-height: 2px;
    transition: height 0.15s ease, background 0.15s ease;
}
.forecast-bar-wrap:hover .forecast-bar { background: var(--text); }
.forecast-tip {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--text);
    color: var(--card);
    padding: 5px 9px;
    border-radius: 6px;
    font-size: 12px;
    line-height: 1.3;
    white-space: nowrap;
    text-align: center;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.12s ease;
    z-index: 5;
}
.forecast-tip strong { font-weight: 700; }
.forecast-tip .tip-span {
    display: block;
    font-size: 10px;
    opacity: 0.7;
}
.forecast-tip::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    border: 4px solid transparent;
    border-top-color: var(--text);
}
.forecast-bar-wrap:hover .forecast-tip { opacity: 1; }
.forecast-axis { display: flex; gap: 3px; margin-top: 6px; }
.forecast-axis .tick {
    flex: 1;
    text-align: center;
    font-size: 10px;
    color: var(--subtext);
    white-space: nowrap;
    overflow: visible;
}
.forecast-total { margin-top: 12px; font-size: 13px; color: var(--subtext); }
.forecast-total strong { color: var(--text); }

.chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 13px;
    color: var(--text);
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
}
.chip:hover { border-color: var(--accent); color: var(--accent); }
.chip-list { display: flex; flex-wrap: wrap; gap: 6px; }

/* ── Lookup modes + kanji finder ─────────────────────────────────── */
.lookup-modes { margin-bottom: 12px; }
.lookup-modes a {
    font-size: 13px;
    font-weight: 500;
    padding: 5px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    color: var(--subtext);
    text-decoration: none;
}
.lookup-modes a:hover { color: var(--text); }
.lookup-modes a.active { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
/* The word being built: one box per kanji. */
.finder-word {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 10px 12px;
    margin-bottom: 14px;
}
.word-slots { display: flex; align-items: center; gap: 6px; flex: 1; flex-wrap: wrap; min-width: 0; }
.word-slots .slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 44px;
    height: 48px;
    padding: 0 4px;
    border-bottom: 2px solid var(--accent);
    font-size: 30px;
    line-height: 1;
}
.word-slots .slot.empty { color: var(--border); border-bottom-color: var(--border); }
.word-actions { display: flex; gap: 6px; align-items: center; margin-left: auto; }
.word-actions button.primary { display: inline-flex; align-items: center; gap: 6px; }
/* Selected parts and their kanji stay in view under the top bar while
   more parts are picked further down. */
.finder-matches {
    position: sticky;
    top: 56px;
    z-index: 5;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: 12px;
    padding: 10px 12px;
    margin-bottom: 14px;
    box-shadow: 0 6px 16px color-mix(in srgb, var(--bg) 70%, transparent);
}
.matches-head { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.matches-head .count { font-size: 12px; margin-left: 4px; }
.matches-head .clear { margin-left: auto; }
.matches-error { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 13px; color: var(--red); }
.kanji-results {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
    max-height: 30vh;
    overflow-y: auto;
}
.kanji-hit {
    width: 52px;
    height: 52px;
    padding: 0;
    font-size: 30px;
    font-weight: 400;
    line-height: 1;
    border: 1px solid var(--border);
    border-radius: 8px;
}
.kanji-hit:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); opacity: 1; }
.part-tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(44px, 1fr));
    gap: 6px;
}
.part-btn {
    height: 44px;
    padding: 0;
    font-size: 22px;
    font-weight: 400;
    line-height: 1;
    border: 1px solid var(--border);
    border-radius: 8px;
}
.part-btn:hover:not(:disabled) { border-color: var(--accent); opacity: 1; }
.part-btn.selected { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
.part-btn.small { width: 36px; height: 36px; min-height: 0; font-size: 18px; }
.part-btn:disabled { opacity: 0.2; }
.all-parts .part-tiles { margin-bottom: 14px; }
.stroke-head {
    margin: 0 0 6px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--subtext);
}
@media (max-width: 540px) {
    .part-tiles { grid-template-columns: repeat(auto-fill, minmax(40px, 1fr)); }
    .part-btn { height: 40px; min-height: 0; font-size: 20px; }
    .kanji-hit { width: 46px; height: 46px; min-height: 0; font-size: 26px; }
}

.toolbar {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-bottom: 16px;
    flex-wrap: wrap;
}
.toolbar input[type="search"] { flex: 1; min-width: 200px; }
.toolbar .count { color: var(--subtext); font-size: 13px; white-space: nowrap; }
/* Word List: filter buttons and the sort menu share one row, phone included. */
.list-controls { flex-wrap: nowrap; }
.list-controls .seg-tabs { flex: 1; flex-wrap: nowrap; }
.list-controls .sort-select {
    width: auto;
    flex: 0 0 auto;
    padding: 6px 10px;
    font-size: 13px;
}

/* ── Forms ───────────────────────────────────────────────────────── */
.form-row {
    display: grid;
    grid-template-columns: 1fr;
    gap: 6px;
    margin-bottom: 14px;
}
.form-row > label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text);
}
.form-row .hint { font-size: 12px; color: var(--subtext); }

/* ── Hero search ─────────────────────────────────────────────────── */
.hero-search input[type="search"] {
    padding: 14px 18px;
    border-radius: 12px;
}

/* ── Review-specific ─────────────────────────────────────────────── */
.review-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 18px 20px;
}
.review-card .face {
    padding: 36px 16px;
    text-align: center;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    margin: 12px 0 16px;
}
.review-card .face .word { font-size: 36px; font-weight: 600; letter-spacing: 1px; }
.review-card .face .kanji-face { font-size: 72px; font-weight: 500; line-height: 1.1; }
.kanji-back { display: flex; flex-direction: column; gap: 6px; align-items: center; text-align: center; font-size: 15px; }
.kanji-back .meanings { font-size: 18px; font-weight: 600; }
.kanji-back .kanji-parts { display: flex; flex-wrap: wrap; justify-content: center; align-items: flex-start; gap: 8px; margin-top: 6px; }
.kanji-back .kanji-part { display: flex; flex-direction: column; align-items: center; gap: 6px; }
.kanji-back .part { display: flex; flex-direction: column; align-items: center; gap: 2px; min-width: 64px; padding: 6px 10px; border: 1px solid var(--border); border-radius: 8px; background: transparent; color: var(--text); font-weight: 400; }
.kanji-back button.part { border-style: dashed; }
.kanji-back button.part.open { border-style: solid; border-color: var(--accent); }
.kanji-back .part .el { font-size: 24px; line-height: 1.1; }
.kanji-back .part .m { max-width: 10em; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; text-align: center; font-size: 12px; color: var(--subtext); }
.kanji-back .mnemonic { margin-top: 8px; max-width: 32em; text-align: center; font-style: italic; color: var(--subtext); }
/* Part roles as inset edge bars, so they combine with each other and with the
   dashed (expandable) / accent (open) border: radical = left, sound = bottom. */
.kanji-back .part { --radical-mark: color-mix(in srgb, var(--blue) 45%, transparent); --sound-mark: color-mix(in srgb, var(--yellow) 45%, transparent); }
.kanji-back .part.radical { box-shadow: inset 2px 0 0 var(--radical-mark); }
.kanji-back .part.sound { box-shadow: inset 0 -2px 0 var(--sound-mark); }
.kanji-back .part.radical.sound { box-shadow: inset 2px 0 0 var(--radical-mark), inset 0 -2px 0 var(--sound-mark); }
.kanji-back .used-in { margin-top: 6px; display: flex; flex-direction: column; align-items: center; gap: 4px; width: 100%; }
.kanji-back .used-word { max-width: 100%; }
.review-card .kanji-facts { font-size: 12px; }
.kanji-back .used-word summary { cursor: pointer; font-size: 17px; list-style-position: inside; }
.kanji-back .used-word-info { display: flex; flex-direction: column; gap: 2px; font-size: 13px; color: var(--subtext); padding: 2px 0 6px; }
.kanji-back .used-word-info .reading { color: var(--text); font-size: 14px; }
.review-card .face .kanji-sub { color: var(--subtext); font-size: 22px; font-weight: 500; margin-top: 6px; letter-spacing: 1px; }
.review-card .face .reading { color: var(--subtext); font-size: 16px; margin-top: 6px; }
.review-card .face .recall-glosses { font-size: 16px; }
.review-card .face .recall-glosses .num { color: var(--subtext); margin-right: 4px; }

.subtabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
    margin: 8px 0 12px;
}
.subtabs button {
    background: transparent;
    color: var(--subtext);
    border-radius: 0;
    border-bottom: 2px solid transparent;
    padding: 8px 12px;
    font-size: 13px;
}
.subtabs button:hover { color: var(--text); background: transparent; opacity: 1; }
.subtabs button.active { color: var(--accent); border-bottom-color: var(--accent); }

.rate-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
}
.rate-grid button { padding: 12px 8px; font-size: 14px; font-weight: 600; }

/* Keyboard-shortcut hints on the review buttons. Only shown where there is a
   real keyboard and pointer; touch devices would just see noise. */
.review-card kbd {
    margin-left: 8px;
    padding: 0 5px;
    border: 1px solid currentColor;
    border-radius: 4px;
    font: 11px/16px ui-monospace, monospace;
    opacity: 0.6;
}
@media (hover: none), (pointer: coarse), (max-width: 540px) { .review-card kbd { display: none; } }

.kanji-row {
    display: flex;
    gap: 14px;
    align-items: center;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
}
.kanji-row .literal {
    font-size: 32px;
    min-width: 48px;
    text-align: center;
    color: var(--accent);
}
.kanji-row .meta { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }

.example-row {
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
}
.example-row .jp { font-size: 15px; }
.example-row .en { color: var(--subtext); font-size: 13px; margin-top: 2px; }
.example-row mark {
    background: color-mix(in srgb, var(--yellow) 60%, transparent);
    color: var(--text);
    padding: 0 3px;
    border-radius: 3px;
}

/* ── Table polish ────────────────────────────────────────────────── */
tbody tr:hover { background: color-mix(in srgb, var(--card) 50%, transparent); }
tbody tr td:first-child { font-weight: 500; }

/* ── Empty state with art ────────────────────────────────────────── */
.empty-state {
    text-align: center;
    padding: 56px 16px;
    color: var(--subtext);
}
.empty-state .glyph {
    display: flex;
    justify-content: center;
    font-size: 44px;
    color: var(--accent);
    opacity: 0.7;
    margin-bottom: 8px;
}
.empty-state .headline {
    font-size: 16px;
    color: var(--text);
    margin-bottom: 4px;
}
.empty-state .helper { font-size: 13px; }
.empty-state button { margin-top: 14px; }

/* ── Inline misc ─────────────────────────────────────────────────── */
.link {
    color: var(--blue);
    text-decoration: none;
}
.link:hover { text-decoration: underline; }

.pill {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    letter-spacing: 0.04em;
    text-indent: 0.04em;
}

.divider {
    border: none;
    border-top: 1px solid var(--border);
    margin: 12px 0;
}

.card.clickable { cursor: pointer; transition: border-color 0.12s, transform 0.04s; }
.card.clickable:hover { border-color: var(--accent); }
.card.clickable:active { transform: translateY(1px); }

/* ── Inline expansion panel under a selected EntryCard ────────────── */
.expansion-panel {
    background: var(--card);
    border: 1px solid var(--accent);
    border-top: none;
    border-radius: 0 0 10px 10px;
    padding: 12px 16px 16px;
    margin-top: -12px;      /* visually pull the panel up to attach to the card above */
    margin-bottom: 12px;
    animation: expand-in 0.16s ease-out;
}
@keyframes expand-in {
    from { opacity: 0; transform: translateY(-4px); }
    to   { opacity: 1; transform: translateY(0); }
}
.expansion-panel .subtabs { margin-top: 0; }
/* When a card is expanded, drop its bottom-rounding so the panel meets flush. */
.card.clickable:has(+ .expansion-panel) {
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
    border-color: var(--accent);
    margin-bottom: 0;
}

/* Detail revealed inside a staged "New Words" card (panel nested under the
   word, not a sibling — so it needs a real top gap + divider, not the
   negative margin the sibling .expansion-panel uses). */
.entry-detail {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
    animation: fade-in 0.16s ease-out;
}
@keyframes fade-in {
    from { opacity: 0; }
    to   { opacity: 1; }
}
"#;
