import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";
import { CARDS_BACKUP_KEY } from "../shared/types.ts";

type Dictionary = InstanceType<typeof JmDictWasm.Dictionary>;

const HL_NAME = "jp-srs-match";
const SKIP_TAGS = new Set([
  "SCRIPT",
  "STYLE",
  "NOSCRIPT",
  "TEXTAREA",
  "INPUT",
  "SELECT",
]);

let dict: Dictionary | null = null;
let srsSequences = new Set<number>();
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let observer: MutationObserver | null = null;
let storageListenerAdded = false;
/** False while lookups are switched off, so no rebuild repaints underlines. */
let active = true;

/**
 * Matches any character `japanese_utils::is_japanese` accepts. Text nodes
 * without one can never contain a known word, so they skip the WASM call.
 */
const JAPANESE_RE =
  /[\u3000-\u30FF\u31F0-\u31FF\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF01-\uFF9F\uFFE0-\uFFEF\u{20000}-\u{2B81F}]/u;

type Match = [start: number, len: number];

/**
 * `find_in_text` results per text node, keyed on the text they were computed
 * for, so a rebuild after a DOM mutation only scans new or changed nodes.
 * Replaced wholesale when the known set changes, since every match depends on
 * it.
 */
let matchCache = new WeakMap<Text, { text: string; matches: Match[] }>();

/** Hand the current known set to WASM and drop matches computed for the old one. */
function pushKnown(): void {
  matchCache = new WeakMap();
  dict?.set_known(Uint32Array.from(srsSequences));
}

function matchesFor(node: Text, text: string): Match[] {
  const cached = matchCache.get(node);
  if (cached && cached.text === text) return cached.matches;
  const matches = JAPANESE_RE.test(text)
    ? ((dict!.find_in_text(text) as Match[] | null) ?? [])
    : [];
  matchCache.set(node, { text, matches });
  return matches;
}

function injectStyle(): void {
  if (document.getElementById("jp-srs-style")) return;
  const s = document.createElement("style");
  s.id = "jp-srs-style";
  s.textContent = `::highlight(${HL_NAME}) { text-decoration: underline 2px rgba(203,166,247,0.7); }`;
  (document.head ?? document.documentElement).appendChild(s);
}

async function rebuildHighlights(): Promise<void> {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  if (!active) return;
  if (!dict || srsSequences.size === 0) {
    CSS.highlights.delete(HL_NAME);
    return;
  }
  try {
    const t0 = performance.now();
    const allRanges: Range[] = [];
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      {
        acceptNode(node: Node) {
          const p = (node as Text).parentElement;
          if (!p) return NodeFilter.FILTER_REJECT;
          if (SKIP_TAGS.has(p.tagName)) return NodeFilter.FILTER_REJECT;
          if (p.isContentEditable) return NodeFilter.FILTER_REJECT;
          if (p.closest("#yomeru-host")) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        },
      },
    );
    let node: Node | null;
    while ((node = walker.nextNode()) !== null) {
      const text = (node as Text).textContent;
      if (!text?.trim()) continue;
      // find_in_text returns UTF-16 code-unit offsets, which is exactly what
      // Range wants for a text node — do not convert these to char indices.
      for (const [start, len] of matchesFor(node as Text, text)) {
        const r = new Range();
        r.setStart(node, start);
        r.setEnd(node, start + len);
        allRanges.push(r);
      }
    }
    if (allRanges.length > 0) {
      CSS.highlights.set(HL_NAME, new Highlight(...allRanges));
    } else {
      CSS.highlights.delete(HL_NAME);
    }
    console.debug(
      `[yomeru] SRS highlight rebuild: ${allRanges.length} matches in ${(performance.now() - t0).toFixed(1)}ms`,
    );
  } catch (e) {
    console.warn("[yomeru] SRS highlight error:", e);
  }
}

export async function initSrsHighlighter(
  dictionary: Dictionary,
): Promise<void> {
  dict = dictionary;
  injectStyle();
  try {
    const res = (await browser.runtime.sendMessage({
      type: "GET_SRS_SEQUENCES",
    })) as { sequences: number[] };
    srsSequences = new Set(res?.sequences ?? []);
  } catch {
    return;
  }
  pushKnown();
  rebuildHighlights();
  if (!storageListenerAdded) {
    storageListenerAdded = true;
    browser.storage.onChanged.addListener((changes, area) => {
      if (area !== "local" || !(CARDS_BACKUP_KEY in changes)) return;
      if (applyCardsChange(changes[CARDS_BACKUP_KEY].newValue)) {
        rebuildHighlights();
      }
    });
  }
  observer = new MutationObserver(() => {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(rebuildHighlights, 500);
  });
  observer.observe(document.body, { childList: true, subtree: true });
}

/**
 * Replaces the known-sequence set from a new cards backup — written after
 * every add, delete and sync, including ones made in another tab or pulled
 * from another device. Returns whether the set changed, so the caller can skip
 * the full-page rescan on the common case (a review, which changes no entry).
 */
export function applyCardsChange(cards: unknown): boolean {
  const next = new Set<number>();
  if (Array.isArray(cards)) {
    for (const c of cards) {
      const seq = (c as { sequence?: unknown } | null)?.sequence;
      if (typeof seq === "number" && Number.isFinite(seq)) next.add(seq);
    }
  }
  const same =
    next.size === srsSequences.size && [...next].every((s) => srsSequences.has(s));
  if (same) return false;
  srsSequences = next;
  pushKnown();
  return true;
}

export function srsSequenceAdded(sequence: number): void {
  if (srsSequences.has(sequence)) return;
  srsSequences.add(sequence);
  pushKnown();
  rebuildHighlights();
}

export function hasSrsSequence(sequence: number): boolean {
  return srsSequences.has(sequence);
}

export function disableSrsHighlighter(): void {
  active = false;
  observer?.disconnect();
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  if (typeof CSS !== "undefined" && CSS.highlights) {
    CSS.highlights.delete(HL_NAME);
  }
}

export function enableSrsHighlighter(): void {
  active = true;
  if (observer && document.body) {
    observer.observe(document.body, { childList: true, subtree: true });
  }
  rebuildHighlights();
}
