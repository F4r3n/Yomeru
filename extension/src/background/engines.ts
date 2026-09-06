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

export type SrsEngine = InstanceType<typeof SrsWasm.SrsEngine>;
export type KanjiDictionary = InstanceType<typeof KanjiWasm.KanjiDictionary>;
export type ExamplesDict = InstanceType<typeof ExamplesWasm.ExamplesDict>;
export type JmDictDictionary = InstanceType<typeof JmDictWasm.Dictionary>;

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
    kanji = new mod.KanjiDictionary(await loadData("kanjidic.bin"));
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

/**
 * Warms the two engines almost every session needs, so the first review or
 * kanji popup doesn't pay the load. Failures are logged and left to the
 * accessors to retry.
 */
export function warmUp(): void {
  getSrs().catch((e) => console.error("[yomeru] srs init failed:", e));
  getKanji().catch((e) => console.error("[yomeru] kanji init failed:", e));
}
