import { mount } from "svelte";
import Popup from "./Popup.svelte";
import { popupStore } from "./popup-store";
import { getJapaneseAtPoint, isEditableAt } from "./detector";
import { initHighlight, setHighlight, clearHighlight } from "./highlight";
import { POPUP_CSS, PIN_DELAY_MS } from "./popup.css";
import {
  initSrsHighlighter,
  disableSrsHighlighter,
  enableSrsHighlighter,
} from "./srs-highlighter";

// ── Types from wasm-pack generated declarations ───────────────────────────────
import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";
import type * as KanjidicWasm from "../../_generated/kanjidic-wasm/kanjidic_wasm.js";
import type { WordEntry } from "../shared/types.ts";

type Dictionary = InstanceType<typeof JmDictWasm.Dictionary>;
type KanjiDictionary = InstanceType<typeof KanjidicWasm.KanjiDictionary>;

// ── WASM state ────────────────────────────────────────────────────────────────

let dictionary: Dictionary | null = null;
let wasmExtractRun: typeof JmDictWasm.extract_japanese_run | null = null;
let kanjiDictionary: KanjiDictionary | null = null;

async function initDictionary(): Promise<void> {
  try {
    const t0 = performance.now();

    const wasmJsUrl = browser.runtime.getURL(
      "_generated/jmdict-wasm/jmdict_wasm.js",
    );
    const wasmBinUrl = browser.runtime.getURL(
      "_generated/jmdict-wasm/jmdict_wasm_bg.wasm",
    );
    const wasm = (await import(
      /* @vite-ignore */ wasmJsUrl
    )) as typeof JmDictWasm;
    await wasm.default(wasmBinUrl);
    const t1 = performance.now();

    wasmExtractRun = wasm.extract_japanese_run;

    //Possible improvement allocate in the WASM memory and write into it
    //Issue in WASM the memory is not released after
    const binUrl = browser.runtime.getURL("data/jmdict.bin");
    const resp = await fetch(binUrl);
    if (!resp.ok) throw new Error(`fetch jmdict.bin: ${resp.status}`);
    const t2 = performance.now();

    const bytes = new Uint8Array(await resp.arrayBuffer());
    const t3 = performance.now();

    dictionary = new wasm.Dictionary(bytes);
    const t4 = performance.now();

    console.log(
      `[yomeru] init: wasm=${(t1 - t0).toFixed(1)}ms fetch=${(t2 - t1).toFixed(1)}ms buffer=${(t3 - t2).toFixed(1)}ms parse=${(t4 - t3).toFixed(1)}ms total=${(t4 - t0).toFixed(1)}ms`,
    );

    initSrsHighlighter(dictionary);
  } catch (e) {
    console.error("[yomeru] Dictionary init failed:", e);
  }
}

async function initKanjiDictionary(): Promise<void> {
  try {
    const wasmJsUrl = browser.runtime.getURL(
      "_generated/kanjidic-wasm/kanjidic_wasm.js",
    );
    const wasmBinUrl = browser.runtime.getURL(
      "_generated/kanjidic-wasm/kanjidic_wasm_bg.wasm",
    );
    const wasm = (await import(
      /* @vite-ignore */ wasmJsUrl
    )) as typeof KanjidicWasm;
    await wasm.default(wasmBinUrl);

    const binUrl = browser.runtime.getURL("data/kanjidic.bin");
    const resp = await fetch(binUrl);
    if (!resp.ok) throw new Error(`fetch kanjidic.bin: ${resp.status}`);
    const bytes = new Uint8Array(await resp.arrayBuffer());

    kanjiDictionary = new wasm.KanjiDictionary(bytes);
  } catch (e) {
    console.error("[yomeru] KanjiDictionary init failed:", e);
  }
}

// ── Shadow DOM + Svelte popup ─────────────────────────────────────────────────

const shadowHost = document.createElement("div");
shadowHost.id = "yomeru-host";
Object.assign(shadowHost.style, {
  position: "fixed",
  zIndex: "2147483647",
  pointerEvents: "none",
  top: "0",
  left: "0",
});
document.documentElement.appendChild(shadowHost);

const shadowRoot = shadowHost.attachShadow({ mode: "closed" });

const styleEl = document.createElement("style");
styleEl.textContent = POPUP_CSS;
shadowRoot.appendChild(styleEl);

mount(Popup, { target: shadowRoot });

// Keep pointer-events in sync with popup visibility.
popupStore.subscribe((state) => {
  shadowHost.style.pointerEvents = state.visible ? "auto" : "none";
});

// ── Highlight init ────────────────────────────────────────────────────────────

initHighlight();

function isJpChar(ch: string): boolean {
  const cp = ch.codePointAt(0) ?? 0;
  return (
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x3041 && cp <= 0x309f) ||
    (cp >= 0x30a0 && cp <= 0x30ff) ||
    cp === 0x30fc
  );
}

// DOM Range uses UTF-16 code unit offsets; Rust uses Unicode code point offsets.
// These two converters bridge the gap for texts containing non-BMP characters
// (e.g. emoji like 🌟 that occupy two UTF-16 code units but one Rust char).

function utf16ToCodePoint(text: string, utf16: number): number {
  let cp = 0;
  let i = 0;
  while (i < utf16 && i < text.length) {
    const code = text.charCodeAt(i);
    i += code >= 0xd800 && code <= 0xdbff ? 2 : 1;
    cp++;
  }
  return cp;
}

function codePointToUtf16(text: string, cp: number): number {
  let i = 0;
  let n = 0;
  while (n < cp && i < text.length) {
    const code = text.charCodeAt(i);
    i += code >= 0xd800 && code <= 0xdbff ? 2 : 1;
    n++;
  }
  return i;
}

// ── Enabled state ─────────────────────────────────────────────────────────────

let enabled = true;

browser.storage.local.get("enabled").then((res) => {
  enabled = (res as { enabled?: boolean }).enabled ?? true;
  if (enabled) ensureDictionaries();
});

browser.storage.onChanged.addListener((changes, area) => {
  if (area !== "local" || !("enabled" in changes)) return;
  enabled = changes.enabled.newValue ?? true;
  if (!enabled) {
    popupStore.forceHide();
    clearHighlight();
    lastLookedUp = null;
    disableSrsHighlighter();
  } else {
    ensureDictionaries();
    enableSrsHighlighter();
  }
});

// ── Hover detection ───────────────────────────────────────────────────────────

let lastLookedUp: string | null = null;
let hideTimer: ReturnType<typeof setTimeout> | null = null;
let hoverTimer: ReturnType<typeof setTimeout> | null = null;
let pinTimer: ReturnType<typeof setTimeout> | null = null;
let wasOverPopup = false;

// composedPath includes shadowHost whenever the event originates inside the
// shadow root (even with mode:"closed"), so this reliably detects hover over
// the popup regardless of the shadow host's zero layout size.
document.addEventListener(
  "mousemove",
  (e) => {
    if (!enabled) return;
    clearTimeout(hoverTimer!);
    if (e.composedPath().includes(shadowHost)) {
      clearTimeout(hideTimer!);
      wasOverPopup = true;
      return;
    }
    // Mouse just left the popup — dismiss it regardless of pin state.
    if (wasOverPopup) {
      wasOverPopup = false;
      popupStore.forceHide();
      clearHighlight();
      lastLookedUp = null;
      return;
    }
    hoverTimer = setTimeout(() => handleHover(e), 60);
  },
  { passive: true },
);

document.addEventListener("mouseleave", () => {
  scheduleHide();
});

async function handleHover(e: MouseEvent): Promise<void> {
  if (!enabled || !dictionary) return;
  if (isEditableAt(e.clientX, e.clientY)) {
    scheduleHide();
    return;
  }

  const hit = getJapaneseAtPoint(e.clientX, e.clientY);
  if (!hit) {
    scheduleHide();
    return;
  }

  // caretPositionFromPoint returns a UTF-16 code unit offset; Rust's
  // extract_japanese_run expects a Unicode code point offset. Convert once
  // and keep cpOffset (code points) for all Rust calls throughout this function.
  let cpOffset = utf16ToCodePoint(hit.nodeText, hit.charOffset);
  let text = wasmExtractRun!(hit.nodeText, cpOffset);

  if (!text && cpOffset > 0) {
    cpOffset -= 1;
    text = wasmExtractRun!(hit.nodeText, cpOffset);
  }
  if (!text) {
    scheduleHide();
    return;
  }

  type LookupResult = {
    entries: WordEntry[];
    match_len: number;
  } | null;
  let cachedResult: LookupResult = null;

  if (cpOffset > 0) {
    const textBack = wasmExtractRun!(hit.nodeText, cpOffset - 1);
    if (textBack) {
      const resultBack = dictionary.lookup_at(textBack) as LookupResult;
      const resultFwd = dictionary.lookup_at(text) as LookupResult;
      if ((resultBack?.match_len ?? 0) > (resultFwd?.match_len ?? 0)) {
        cpOffset -= 1;
        text = textBack;
        cachedResult = resultBack;
      } else {
        cachedResult = resultFwd;
      }
    }
  }

  clearTimeout(hideTimer!);

  try {
    const result = (cachedResult ?? dictionary.lookup_at(text)) as LookupResult;
    if (!result?.entries?.length) {
      scheduleHide();
      return;
    }

    const hw =
      result.entries[0].kanji_forms?.[0]?.text ??
      result.entries[0].reading_forms?.[0]?.text ??
      text;

    // Always refine the highlight to the exact dictionary match length,
    // even when the word hasn't changed (phase 1 used the full run).
    const utf16Start = codePointToUtf16(hit.nodeText, cpOffset);
    const utf16End = codePointToUtf16(
      hit.nodeText,
      cpOffset + (result.match_len ?? 0),
    );

    const wordRange = new Range();
    wordRange.setStart(hit.node, utf16Start);
    wordRange.setEnd(hit.node, utf16End);
    const wordRect = wordRange.getBoundingClientRect();

    setHighlight(hit.node, utf16Start, utf16End - utf16Start);

    if (hw === lastLookedUp) return;
    lastLookedUp = hw;

    const kanjiEntries = kanjiDictionary
      ? ((kanjiDictionary.lookup_many(
          hw,
        ) as import("../shared/types.ts").KanjiEntry[]) ?? [])
      : [];

    popupStore.show(
      result.entries as unknown as import("../shared/types.ts").WordEntry[],
      kanjiEntries,
      wordRect.width > 0 ? wordRect.left  : e.clientX,
      wordRect.width > 0 ? wordRect.right : e.clientX,
      wordRect.height > 0 ? wordRect.top    : e.clientY - 20,
      wordRect.height > 0 ? wordRect.bottom : e.clientY,
    );

    // After 3s of hovering the same word the popup becomes sticky.
    clearTimeout(pinTimer!);
    pinTimer = setTimeout(() => popupStore.pin(), PIN_DELAY_MS);

    browser.runtime.sendMessage({
      type: "LOG_LOOKUP",
      payload: {
        word: hw,
        reading: result.entries[0].reading_forms?.[0]?.text ?? "",
      },
    });
  } catch (err) {
    console.error("[yomeru] Lookup error:", err);
  }
}

function scheduleHide(): void {
  clearTimeout(pinTimer!);
  clearTimeout(hideTimer!);
  // 150 ms grace period so the mouse can travel from the word to the popup.
  hideTimer = setTimeout(() => {
    popupStore.forceHide();
    clearHighlight();
    lastLookedUp = null;
  }, 150);
}

// ── Selection lookup ──────────────────────────────────────────────────────────

document.addEventListener("mouseup", async (e) => {
  if (!enabled || !dictionary) return;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed) return;
  const text = sel.toString().trim();
  if (!text || text.length > 50 || ![...text].some((c) => isJpChar(c))) return;

  try {
    const allEntries = dictionary.lookup(text) as WordEntry[];
    // Keep only entries where the selected text is the primary kanji or reading form.
    // This avoids showing unrelated entries that merely list the selected kanji as
    // an obscure alternative spelling.
    const primary = allEntries.filter(
      (e) =>
        e.kanji_forms[0]?.text === text ||
        (e.kanji_forms.length === 0 && e.reading_forms[0]?.text === text),
    );
    const entries = primary.length > 0 ? primary : allEntries;
    if (entries?.length) {
      const kanjiEntries = kanjiDictionary
        ? ((kanjiDictionary.lookup_many(
            text,
          ) as import("../shared/types.ts").KanjiEntry[]) ?? [])
        : [];
      const selRect = sel.rangeCount > 0 ? sel.getRangeAt(0).getBoundingClientRect() : null;
      popupStore.show(
        entries as unknown as import("../shared/types.ts").WordEntry[],
        kanjiEntries,
        selRect?.width  ? selRect.left   : e.clientX,
        selRect?.width  ? selRect.right  : e.clientX,
        selRect?.height ? selRect.top    : e.clientY - 20,
        selRect?.height ? selRect.bottom : e.clientY,
      );
      lastLookedUp = text;
    }
  } catch (err) {
    console.error("[yomeru] Selection lookup error:", err);
  }
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    clearTimeout(pinTimer!);
    popupStore.forceHide();
    clearHighlight();
    lastLookedUp = null;
  }
});

// ── Lazy init (triggered on first hover / selection) ─────────────────────────

let dictPromise: Promise<void> | null = null;
let kanjiPromise: Promise<void> | null = null;

function ensureDictionaries(): Promise<void> {
  if (!dictPromise) {
    kanjiPromise ??= initKanjiDictionary();
    dictPromise = initDictionary();
  }
  return dictPromise;
}
