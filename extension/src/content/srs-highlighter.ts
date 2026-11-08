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

/**
 * Text far from the viewport isn't scanned until it gets close: each rebuild
 * only runs `find_in_text` on nodes whose parent element is in `nearElements`,
 * and `viewport` watches the parents of the rest. Once scanned, a node's
 * matches stay cached and painted, so scrolling away never drops underlines.
 */
const VIEWPORT_MARGIN = "100% 0px";
/** Delay before scanning text that just scrolled near the viewport. */
const NEAR_SCAN_DELAY_MS = 50;

let viewport: IntersectionObserver | null = null;
/** Elements `viewport` is watching: parents of unscanned Japanese text. */
const watched = new Set<Element>();
/** Watched elements that came near the viewport since the last rebuild. */
const nearElements = new Set<Element>();
let nearTimer: ReturnType<typeof setTimeout> | null = null;

function createViewportObserver(): IntersectionObserver | null {
  if (typeof IntersectionObserver === "undefined") return null;
  return new IntersectionObserver(
    (entries) => {
      let anyNear = false;
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        // One scan per approach: unwatch now, and re-watch only if its text
        // changes later while away from the viewport.
        nearElements.add(e.target);
        viewport?.unobserve(e.target);
        watched.delete(e.target);
        anyNear = true;
      }
      if (anyNear && nearTimer === null) {
        nearTimer = setTimeout(() => {
          nearTimer = null;
          rebuildHighlights();
        }, NEAR_SCAN_DELAY_MS);
      }
    },
    { rootMargin: VIEWPORT_MARGIN },
  );
}

function watch(el: Element): void {
  if (!viewport || watched.has(el)) return;
  watched.add(el);
  viewport.observe(el);
}

/** Stop watching elements the page removed, so they can be collected. */
function sweepWatched(): void {
  for (const el of watched) {
    if (!el.isConnected) {
      viewport?.unobserve(el);
      watched.delete(el);
    }
  }
}

function stopViewportTracking(): void {
  viewport?.disconnect();
  watched.clear();
  nearElements.clear();
  if (nearTimer !== null) {
    clearTimeout(nearTimer);
    nearTimer = null;
  }
}

/**
 * Matches for `node`, or `null` when it is Japanese text that hasn't been
 * scanned and isn't near the viewport yet (its parent is then watched).
 */
function matchesFor(node: Text, text: string): Match[] | null {
  const cached = matchCache.get(node);
  if (cached && cached.text === text) return cached.matches;
  if (!JAPANESE_RE.test(text)) {
    matchCache.set(node, { text, matches: [] });
    return [];
  }
  const parent = node.parentElement!;
  if (viewport && !nearElements.has(parent)) {
    watch(parent);
    return null;
  }
  const matches = (dict!.find_in_text(text) as Match[] | null) ?? [];
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
    viewport ??= createViewportObserver();
    sweepWatched();
    const allRanges: Range[] = [];
    let deferred = 0;
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
      const matches = matchesFor(node as Text, text);
      if (matches === null) {
        deferred++;
        continue;
      }
      for (const [start, len] of matches) {
        const r = new Range();
        r.setStart(node, start);
        r.setEnd(node, start + len);
        allRanges.push(r);
      }
    }
    // Every near element's text was just visited; anything still near later
    // will be reported again by `viewport` once it is re-watched.
    nearElements.clear();
    if (allRanges.length > 0) {
      CSS.highlights.set(HL_NAME, new Highlight(...allRanges));
    } else {
      CSS.highlights.delete(HL_NAME);
    }
    console.debug(
      `[yomeru] SRS highlight rebuild: ${allRanges.length} matches, ${deferred} text nodes waiting for the viewport, ${(performance.now() - t0).toFixed(1)}ms`,
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
  stopViewportTracking();
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
