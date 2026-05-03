import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";

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
let srsWords: string[] = [];
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

function injectStyle(): void {
  if (document.getElementById("jp-srs-style")) return;
  const s = document.createElement("style");
  s.id = "jp-srs-style";
  s.textContent = `::highlight(${HL_NAME}) { text-decoration: underline 2px rgba(203,166,247,0.7); }`;
  (document.head ?? document.documentElement).appendChild(s);
}

async function rebuildHighlights(): Promise<void> {
  if (typeof CSS === "undefined" || !CSS.highlights) return;
  if (!dict || srsWords.length === 0) {
    CSS.highlights.delete(HL_NAME);
    return;
  }
  try {
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
          if (p.closest("#jp-reader-host")) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        },
      },
    );
    let node: Node | null;
    while ((node = walker.nextNode()) !== null) {
      const text = (node as Text).textContent;
      if (!text?.trim()) continue;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const matches = dict.find_in_text(text, srsWords as any) as [
        number,
        number,
      ][];
      for (const [start, len] of matches ?? []) {
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
  } catch (e) {
    console.warn("[jp-reader] SRS highlight error:", e);
  }
}

export async function initSrsHighlighter(
  dictionary: Dictionary,
): Promise<void> {
  dict = dictionary;
  injectStyle();
  try {
    const res = (await browser.runtime.sendMessage({
      type: "GET_SRS_WORDS",
    })) as { words: string[] };
    srsWords = res?.words ?? [];
  } catch {
    return;
  }
  rebuildHighlights();
  new MutationObserver(() => {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(rebuildHighlights, 500);
  }).observe(document.body, { childList: true, subtree: true });
}

export function srsWordAdded(word: string): void {
  if (srsWords.includes(word)) return;
  srsWords.push(word);
  rebuildHighlights();
}
