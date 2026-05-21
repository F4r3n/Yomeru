//! Global CSS — Catppuccin Mocha palette, mirroring the extension's
//! Options.svelte `:global(:root)` block. Inlined as a `<style>` tag.

pub const GLOBAL_CSS: &str = r#"
:root {
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
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
    background: var(--bg);
    color: var(--text);
    font-family: "Noto Sans JP", "Segoe UI", sans-serif;
    font-size: 14px;
    min-height: 100vh;
}
.app-shell {
    max-width: 720px;
    margin: 0 auto;
    padding: 0 0 32px;
}
header {
    background: var(--surface);
    padding: 16px 16px 0;
    border-bottom: 1px solid var(--border);
}
.header-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
}
h1 { font-size: 18px; font-weight: 700; color: var(--blue); }
nav { display: flex; gap: 4px; align-items: stretch; flex-wrap: wrap; }
.tab {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--subtext);
    cursor: pointer;
    padding: 8px 14px;
    font-size: 13px;
    transition: color 0.15s, border-color 0.15s;
    text-decoration: none;
}
.tab:hover { color: var(--text); }
.tab.active { color: var(--accent); border-bottom-color: var(--accent); }
main { padding: 16px; }

button {
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
    padding: 6px 12px;
    background: var(--surface);
    color: var(--text);
    transition: opacity 0.15s, background 0.15s;
}
button:hover:not(:disabled) { opacity: 0.85; }
button:disabled { opacity: 0.4; cursor: not-allowed; }
button.primary { background: var(--accent); color: var(--bg); }
button.danger  { background: var(--red);    color: var(--bg); }
button.success { background: var(--green);  color: var(--bg); }

input, select, textarea {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 14px;
    font-family: inherit;
    width: 100%;
}
input:focus, select:focus, textarea:focus {
    outline: none;
    border-color: var(--accent);
}

.card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px 14px;
    margin-bottom: 10px;
}

.row { display: flex; gap: 8px; align-items: center; }
.col { display: flex; flex-direction: column; gap: 8px; }
.muted { color: var(--subtext); }
.error { color: var(--red); }
.ok { color: var(--green); }

.headword { font-size: 22px; font-weight: 600; color: var(--text); }
.reading  { color: var(--subtext); font-size: 14px; margin-top: 2px; }
.pos      { color: var(--yellow);  font-size: 12px; margin-top: 4px; font-style: italic; }
.gloss    { color: var(--text);    font-size: 13px; margin-top: 2px; }

table { width: 100%; border-collapse: collapse; }
th, td { padding: 8px 6px; text-align: left; border-bottom: 1px solid var(--border); font-size: 13px; }
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
.badge.staging { background: var(--yellow); color: var(--bg); }
.badge.active  { background: var(--green);  color: var(--bg); }
.badge.due     { background: var(--red);    color: var(--bg); }
.badge.new     { background: var(--blue);   color: var(--bg); }

.loading { color: var(--subtext); padding: 16px; text-align: center; }
.empty   { color: var(--subtext); padding: 24px; text-align: center; }
"#;
