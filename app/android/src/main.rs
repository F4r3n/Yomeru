use std::rc::Rc;

use dioxus::prelude::*;
use yomeru_shared::local_dict::{DictAssets, LocalDict};
use yomeru_shared::{launch_with, platform_with_dict};

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
    let dict = LocalDict::new(Rc::new(DictAssets {
        jmdict: JMDICT,
        kanjidic: KANJIDIC,
        examples: EXAMPLES,
        mnemonics: Some(MNEMONICS),
        part_names: Some(PART_NAMES),
    }));
    launch_with(platform_with_dict(Rc::new(dict)));
}
