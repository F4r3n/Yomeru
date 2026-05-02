import { mount } from "svelte";
import Popup from "./Popup.svelte";
import { popupStore } from "./popup-store";
import { getJapaneseAtPoint, isEditableAt } from "./detector";
import { initHighlight, setHighlight, clearHighlight } from "./highlight";
import { POPUP_CSS, PIN_DELAY_MS } from "./popup.css";

// ── Types from wasm-pack generated declarations ───────────────────────────────
import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";

type Dictionary = InstanceType<typeof JmDictWasm.Dictionary>;

// ── WASM state ────────────────────────────────────────────────────────────────

let dictionary: Dictionary | null = null;
let wasmExtractRun: typeof JmDictWasm.extract_japanese_run | null = null;

async function initDictionary(): Promise<void> {
  try {
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

    wasmExtractRun = wasm.extract_japanese_run;

    const binUrl = browser.runtime.getURL("data/jmdict.bin");
    const resp = await fetch(binUrl);
    if (!resp.ok) throw new Error(`fetch jmdict.bin: ${resp.status}`);
    const bytes = new Uint8Array(await resp.arrayBuffer());

    dictionary = new wasm.Dictionary(bytes);
  } catch (e) {
    console.error("[jp-reader] Dictionary init failed:", e);
  }
}

// ── Shadow DOM + Svelte popup ─────────────────────────────────────────────────

const shadowHost = document.createElement("div");
shadowHost.id = "jp-reader-host";
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

// ── JS fallback for extract_japanese_run (before WASM loads) ─────────────────

function jsExtractRun(text: string, charOffset: number): string {
  const chars = [...text];
  if (charOffset >= chars.length || !isJpChar(chars[charOffset])) return "";
  let end = charOffset + 1;
  while (end < chars.length && isJpChar(chars[end])) end++;
  return chars.slice(charOffset, end).join("");
}

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

function extractRunAt(text: string, charOffset: number): string {
  return wasmExtractRun
    ? wasmExtractRun(text, charOffset)
    : jsExtractRun(text, charOffset);
}

// ── Hover detection ───────────────────────────────────────────────────────────

let lastLookedUp: string | null = null;
let hideTimer: ReturnType<typeof setTimeout> | null = null;
let hoverTimer: ReturnType<typeof setTimeout> | null = null;
let pinTimer: ReturnType<typeof setTimeout> | null = null;
let overPopup = false;

// Stop hover-hide logic from firing while the mouse is inside the popup.
shadowHost.addEventListener("mouseenter", () => {
  overPopup = true;
  clearTimeout(hideTimer!);
  clearTimeout(hoverTimer!);
});
shadowHost.addEventListener("mouseleave", () => {
  overPopup = false;
  if (!popupStore.isPinned()) scheduleHide();
});

document.addEventListener(
  "mousemove",
  (e) => {
    if (overPopup) return;
    clearTimeout(hoverTimer!);
    hoverTimer = setTimeout(() => handleHover(e), 120);
  },
  { passive: true },
);

document.addEventListener("mouseleave", () => {
  if (!popupStore.isPinned()) scheduleHide();
});

async function handleHover(e: MouseEvent): Promise<void> {
  if (!dictionary) return;
  if (isEditableAt(e.clientX, e.clientY)) {
    scheduleHide();
    return;
  }

  const hit = getJapaneseAtPoint(e.clientX, e.clientY);
  if (!hit) {
    scheduleHide();
    return;
  }

  // caretPositionFromPoint places the caret *between* characters, so the
  // offset often lands one past the intended character. Try offset-1 as well
  // and keep whichever produces the longer match.
  let charOffset = hit.charOffset;
  let text = extractRunAt(hit.nodeText, charOffset);

  if (!text && charOffset > 0) {
    charOffset -= 1;
    text = extractRunAt(hit.nodeText, charOffset);
  }
  if (!text) {
    scheduleHide();
    return;
  }

  if (charOffset > 0) {
    const textBack = extractRunAt(hit.nodeText, charOffset - 1);
    if (textBack) {
      const resultBack = dictionary.lookup_at(textBack) as {
        entries: JmDictWasm.WordEntry[];
        match_len: number;
      } | null;
      const resultFwd = dictionary.lookup_at(text) as {
        entries: JmDictWasm.WordEntry[];
        match_len: number;
      } | null;
      if ((resultBack?.match_len ?? 0) > (resultFwd?.match_len ?? 0)) {
        charOffset -= 1;
        text = textBack;
      }
    }
  }

  clearTimeout(hideTimer!);

  try {
    const result = dictionary.lookup_at(text) as {
      entries: JmDictWasm.WordEntry[];
      match_len: number;
    } | null;
    if (!result?.entries?.length) {
      scheduleHide();
      return;
    }

    const hw =
      result.entries[0].kanji_forms?.[0]?.text ??
      result.entries[0].reading_forms?.[0]?.text ??
      text;
    if (hw === lastLookedUp) return;

    lastLookedUp = hw;
    setHighlight(hit.node, charOffset, result.match_len ?? 0);
    popupStore.show(
      result.entries as unknown as import("../shared/types.ts").WordEntry[],
      e.clientX,
      e.clientY,
    );

    // After 5 s of hovering the same word the popup becomes sticky.
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
    console.error("[jp-reader] Lookup error:", err);
  }
}

function scheduleHide(): void {
  clearTimeout(pinTimer!);
  clearTimeout(hideTimer!);
  hideTimer = setTimeout(() => {
    popupStore.hide(); // no-op when pinned
    if (!popupStore.isPinned()) {
      clearHighlight();
      lastLookedUp = null;
    }
  }, 300);
}

// ── Selection lookup ──────────────────────────────────────────────────────────

document.addEventListener("mouseup", async (e) => {
  if (!dictionary) return;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed) return;
  const text = sel.toString().trim();
  if (!text || text.length > 50 || ![...text].some((c) => isJpChar(c))) return;

  try {
    const entries = dictionary.lookup(text) as JmDictWasm.WordEntry[];
    if (entries?.length) {
      popupStore.show(
        entries as unknown as import("../shared/types.ts").WordEntry[],
        e.clientX,
        e.clientY,
      );
      lastLookedUp = text;
    }
  } catch (err) {
    console.error("[jp-reader] Selection lookup error:", err);
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

// ── Boot ──────────────────────────────────────────────────────────────────────

initDictionary();
