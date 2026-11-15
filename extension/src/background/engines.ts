/**
 * Lazy loading of the four WASM engines.
 *
 * Each is fetched on first use and then cached for the life of the service
 * worker. Callers go through the `get*` accessors rather than touching the
 * instances, so "load it if it isn't loaded" can't be forgotten at a call site.
 *
 * The examples dictionary is the one that may legitimately be absent — its data
 * file is optional in a build — so its accessor returns null instead of
 * throwing, and one failure marks it unavailable rather than retrying on every
 * lookup.
 */

import type * as SrsWasm from "../../_generated/srs-wasm/srs_wasm.js";
import type * as KanjiWasm from "../../_generated/kanjidic-wasm/kanjidic_wasm.js";
import type * as ExamplesWasm from "../../_generated/examples-wasm/examples_wasm.js";
import type * as JmDictWasm from "../../_generated/jmdict-wasm/jmdict_wasm.js";
import type * as GrammarWasm from "../../_generated/grammar-wasm/grammar_wasm.js";
import { loadKanjiOverlays } from "../shared/kanji-overlays.ts";

export type SrsEngine = InstanceType<typeof SrsWasm.SrsEngine>;
export type KanjiDictionary = InstanceType<typeof KanjiWasm.KanjiDictionary>;
export type ExamplesDict = InstanceType<typeof ExamplesWasm.ExamplesDict>;
export type JmDictDictionary = InstanceType<typeof JmDictWasm.Dictionary>;
export type GrammarDictionary = InstanceType<typeof GrammarWasm.GrammarDictionary>;

/**
 * Loads a wasm-bindgen module and initialises it against its `_bg.wasm`.
 *
 * The `@vite-ignore` matters: `jsUrl` is an extension URL resolved at runtime,
 * so Vite must leave the import alone rather than trying to resolve it at build
 * time.
 */
async function loadModule<M>(dir: string, stem: string): Promise<M> {
  const jsUrl = browser.runtime.getURL(`_generated/${dir}/${stem}.js`);
  const binUrl = browser.runtime.getURL(`_generated/${dir}/${stem}_bg.wasm`);
  const mod = (await import(/* @vite-ignore */ jsUrl)) as M;
  await (mod as { default: (u: string) => Promise<unknown> }).default(binUrl);
  return mod;
}

/** Fetches one of the prebuilt binary indexes shipped in `data/`. */
async function loadData(file: string): Promise<Uint8Array> {
  const url = browser.runtime.getURL(`data/${file}`);
  const buf = await fetch(url).then((r) => r.arrayBuffer());
  return new Uint8Array(buf);
}

let srs: SrsEngine | null = null;
let kanji: KanjiDictionary | null = null;
let examplesDict: ExamplesDict | null = null;
let examplesUnavailable = false;
let jmdict: JmDictDictionary | null = null;

export async function getSrs(): Promise<SrsEngine> {
  if (!srs) {
    const mod = await loadModule<typeof SrsWasm>("srs-wasm", "srs_wasm");
    srs = new mod.SrsEngine();
  }
  return srs;
}

export async function getKanji(): Promise<KanjiDictionary> {
  if (!kanji) {
    const mod = await loadModule<typeof KanjiWasm>(
      "kanjidic-wasm",
      "kanjidic_wasm",
    );
    const dict = new mod.KanjiDictionary(await loadData("kanjidic.bin"));
    await loadKanjiOverlays(dict);
    kanji = dict;
  }
  return kanji;
}

/** Null when the examples data isn't present in this build. */
export async function getExamples(): Promise<ExamplesDict | null> {
  if (examplesDict || examplesUnavailable) return examplesDict;
  try {
    const mod = await loadModule<typeof ExamplesWasm>(
      "examples-wasm",
      "examples_wasm",
    );
    examplesDict = new mod.ExamplesDict(await loadData("examples.bin"));
  } catch {
    examplesUnavailable = true;
  }
  return examplesDict;
}

export async function getJmdict(): Promise<JmDictDictionary> {
  if (!jmdict) {
    const mod = await loadModule<typeof JmDictWasm>(
      "jmdict-wasm",
      "jmdict_wasm",
    );
    jmdict = new mod.Dictionary(await loadData("jmdict.bin"));
  }
  return jmdict;
}

// ── Grammar dictionary ───────────────────────────────────────────────────────
//
// Unlike the others it isn't shipped: the options page downloads or imports
// it and stores the converted `grammar.bin` in this origin's IndexedDB
// (`yomeru_core::grammar`), so it may be absent, and may change while the
// background runs. The options page announces each install or removal on a
// BroadcastChannel, which drops what was loaded.

const GRAMMAR_DB = "yomeru-grammar";
const GRAMMAR_DB_VERSION = 1;
const GRAMMAR_STORE = "files";
const GRAMMAR_KEY = "grammar.bin";
/** `yomeru_core::grammar::CHANGE_CHANNEL`. */
const GRAMMAR_CHANNEL = "yomeru-grammar";

/** The load in progress or done; null to load again on next use. */
let grammar: Promise<GrammarDictionary | null> | null = null;

try {
  new BroadcastChannel(GRAMMAR_CHANNEL).onmessage = () => {
    grammar?.then((d) => d?.free()).catch(() => {});
    grammar = null;
  };
} catch (e) {
  console.error("[yomeru] grammar change channel:", e);
}

/** The stored `grammar.bin`, or null when none is installed. */
function readGrammarBin(): Promise<Uint8Array | null> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(GRAMMAR_DB, GRAMMAR_DB_VERSION);
    // Same schema as the Rust side creates: whichever opens first makes it.
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(GRAMMAR_STORE)) db.createObjectStore(GRAMMAR_STORE);
    };
    req.onerror = () => reject(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction(GRAMMAR_STORE, "readonly").objectStore(GRAMMAR_STORE).get(GRAMMAR_KEY);
      get.onerror = () => {
        db.close();
        reject(get.error);
      };
      get.onsuccess = () => {
        db.close();
        resolve(get.result instanceof Uint8Array ? get.result : null);
      };
    };
  });
}

async function loadGrammar(): Promise<GrammarDictionary | null> {
  const bytes = await readGrammarBin();
  if (!bytes) return null;
  const mod = await loadModule<typeof GrammarWasm>("grammar-wasm", "grammar_wasm");
  return new mod.GrammarDictionary(bytes);
}

/**
 * Null when no grammar dictionary is installed, or the stored one can't be
 * read (an older format: the options page asks to import it again). A
 * failure isn't retried until the next install or removal.
 */
export function getGrammar(): Promise<GrammarDictionary | null> {
  grammar ??= loadGrammar().catch((e) => {
    console.error("[yomeru] grammar dictionary load failed:", e);
    return null;
  });
  return grammar;
}

/**
 * Warms the two engines almost every session needs, so the first review or
 * kanji popup doesn't pay the load. Failures are logged and left to the
 * accessors to retry.
 */
export function warmUp(): void {
  getSrs().catch((e) => console.error("[yomeru] srs init failed:", e));
  getKanji().catch((e) => console.error("[yomeru] kanji init failed:", e));
}
