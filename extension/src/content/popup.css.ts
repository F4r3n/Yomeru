export const PIN_DELAY_MS = 300;

export const POPUP_CSS = `
.jp-popup {
  position: fixed; max-width: 380px; min-width: 220px;
  display: flex; flex-direction: column; overflow: hidden;
  background: #1e1e2e; color: #cdd6f4;
  border: 1px solid #45475a; border-radius: 8px;
  padding: 10px 14px;
  font-family: "Noto Sans JP", "Hiragino Sans", "Yu Gothic", sans-serif;
  font-size: 13px; line-height: 1.5;
  box-shadow: 0 4px 20px rgba(0,0,0,0.5);
  pointer-events: auto; z-index: 2147483647;
}
.jp-content { overflow-y: auto; flex: 1; min-height: 0; }
.jp-header { display: flex; align-items: baseline; gap: 6px; margin-bottom: 4px; flex-wrap: wrap; }
.jp-word   { font-size: 22px; font-weight: 600; color: #89dceb; }
.jp-reading { color: #a6e3a1; font-size: 14px; }
.jp-pos-group { margin-left: auto; }
.jp-pos { font-size: 10px; background: #313244; color: #cba6f7;
          border-radius: 3px; padding: 1px 5px; margin-left: 3px; }
.jp-senses { margin: 4px 0 8px; }
.jp-gloss  { color: #cdd6f4; margin: 2px 0; }
.jp-num    { color: #6c7086; margin-right: 4px; }
.jp-divider { border: none; border-top: 1px solid #313244; margin: 8px 0; }
.jp-add-btn {
  background: #313244; border: 1px solid #45475a; color: #cba6f7;
  border-radius: 4px; padding: 3px 10px; cursor: pointer; font-size: 11px;
}
.jp-add-btn:hover { background: #45475a; }
.jp-add-btn:disabled { opacity: 0.5; cursor: default; }

.jp-pin-ring {
  position: absolute; top: 7px; right: 7px;
  line-height: 0; pointer-events: none;
}
.jp-ring-track {
  fill: none; stroke: #313244; stroke-width: 2;
}
.jp-ring-fill {
  fill: none; stroke: #cba6f7; stroke-width: 2;
  stroke-dasharray: 43.98; stroke-dashoffset: 43.98;
  animation: jp-ring-fill ${PIN_DELAY_MS / 1000}s linear forwards;
  transform-origin: 9px 9px; transform: rotate(-90deg);
}
@keyframes jp-ring-fill {
  to { stroke-dashoffset: 0; }
}
.jp-pin-dot { fill: #cba6f7; }
.jp-tabs { display:flex; border-bottom:1px solid #313244; margin-bottom:8px; }
.jp-tabs--bottom { border-bottom:none; border-top:1px solid #313244; margin-bottom:0; margin-top:8px; }
.jp-tab {
  background:none; border:none; border-bottom:2px solid transparent;
  color:#6c7086; padding:4px 12px; cursor:pointer; font-size:12px;
  font-family:inherit; margin-bottom:-1px;
}
.jp-tabs--bottom .jp-tab { margin-bottom:0; margin-top:-1px; border-bottom:none; border-top:2px solid transparent; }
.jp-tab:hover { color:#cdd6f4; }
.jp-tab--active { color:#cdd6f4; border-bottom-color:#cba6f7; }
.jp-tabs--bottom .jp-tab--active { border-bottom-color:transparent; border-top-color:#cba6f7; }
.jp-kanji-list { display:flex; flex-direction:column; gap:8px; }
.jp-kanji-entry { padding:6px 0; border-top:1px solid #313244; }
.jp-kanji-entry:first-child { border-top:none; padding-top:0; }
.jp-kanji-header { display:flex; align-items:baseline; gap:8px; margin-bottom:4px; }
.jp-kanji-char { font-size:32px; font-weight:600; color:#89dceb; line-height:1; }
.jp-kanji-meta { display:flex; align-items:center; gap:6px; }
.jp-kanji-strokes { font-size:11px; color:#6c7086; }
.jp-kanji-jlpt { font-size:10px; background:#313244; color:#a6e3a1; border-radius:3px; padding:1px 5px; }
.jp-kanji-readings { font-size:12px; color:#cdd6f4; margin:2px 0; }
.jp-kanji-rdlabel { color:#6c7086; margin-right:4px; font-size:11px; }
.jp-kanji-meanings { font-size:12px; color:#a6e3a1; margin-top:3px; }
.jp-corpus { display:flex; flex-direction:column; gap:12px; padding:4px 0; }
.jp-corpus-ex { display:flex; flex-direction:column; gap:3px; }
.jp-corpus-jp { font-size:15px; color:#cdd6f4; line-height:1.6; }
.jp-corpus-mark { background:rgba(203,166,247,0.18); color:#cba6f7; border-radius:2px; padding:0 1px; }
.jp-corpus-en { font-size:13px; color:#6c7086; line-height:1.5; }
.jp-corpus-empty { font-size:13px; color:#6c7086; padding:8px 0; }
`;
