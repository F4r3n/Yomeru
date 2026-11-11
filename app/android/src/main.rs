use std::sync::Arc;

use dioxus::prelude::*;
use yomeru_shared::local_dict::{DictAssets, LocalDict};
use yomeru_shared::{launch_with_config, platform_with_dict, startup_head};

// The dictionaries ship inside the APK, so lookups work offline. The files
// are hard-linked into assets/dict/ from extension/data/ by
// `podman-build.sh` (they're build output of `cargo xtask build-all`, not
// checked in). `asset!` needs them present at compile time.
static JMDICT: Asset = asset!("/assets/dict/jmdict.bin");
static KANJIDIC: Asset = asset!("/assets/dict/kanjidic.bin");
static EXAMPLES: Asset = asset!("/assets/dict/examples.bin");
static MNEMONICS: Asset = asset!("/assets/dict/kanji-mnemonics.tsv");
static PART_NAMES: Asset = asset!("/assets/dict/part-names.tsv");

fn main() {
    let dict = LocalDict::new(Arc::new(DictAssets {
        jmdict: JMDICT,
        kanjidic: KANJIDIC,
        examples: EXAMPLES,
        mnemonics: Some(MNEMONICS),
        part_names: Some(PART_NAMES),
    }));
    // The stylesheet and the saved theme go into the page's <head>, so its
    // first frame is already styled: no white buttons fading in at start-up.
    let config = dioxus::mobile::Config::new().with_custom_head(startup_head());
    launch_with_config(platform_with_dict(Arc::new(dict)), config);
}
