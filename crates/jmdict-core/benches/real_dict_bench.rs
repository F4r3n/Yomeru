//! Benchmarks `lookup_by_sequence` against the *real* JMdict binary so entry
//! sizes are representative (real entries have many senses/glosses/tags, unlike
//! the tiny synthetic entries in `lookup_bench`). Skips gracefully if the
//! binary hasn't been built yet.
//!
//! Run: `cargo bench -p jmdict-core --bench real_dict_bench`

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::OnceLock;

const DEFAULT_BIN_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../extension/data/jmdict.bin");

/// The dictionary binary must be built with the **same** `jmdict-types/full`
/// setting as this bench's reader (benches enable `full` via the dev-dep), or
/// postcard — which isn't self-describing — will fail to decode every entry and
/// the bench will find no sequences. Override the path with `JMDICT_BIN` to
/// point at a matching build.
fn bin_path() -> String {
    std::env::var("JMDICT_BIN").unwrap_or_else(|_| DEFAULT_BIN_PATH.to_string())
}

/// Loads the real dict once and returns a spread of sequences that actually
/// resolve to entries, or `None` if the binary is missing.
fn real_seqs() -> Option<&'static Vec<u32>> {
    static SEQS: OnceLock<Option<Vec<u32>>> = OnceLock::new();
    SEQS.get_or_init(|| {
        let path = bin_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skipping: cannot read {path}: {e}");
                return None;
            }
        };
        jmdict_core::init(&bytes).expect("real dict init failed");
        // Probe a wide ent_seq range and keep the hits — a realistic mix of
        // entry sizes, like the batch the Word List resolves.
        let seqs: Vec<u32> = (1_000_000..9_000_000)
            .step_by(7)
            .filter(|&s| jmdict_core::lookup_by_sequence(s).is_some())
            .take(2000)
            .collect();
        eprintln!("real_dict_bench: collected {} sequences", seqs.len());
        (!seqs.is_empty()).then_some(seqs)
    })
    .as_ref()
}

fn bench_real_by_sequence(c: &mut Criterion) {
    let Some(seqs) = real_seqs() else {
        eprintln!("skipping real_dict_bench: {} not found", bin_path());
        return;
    };

    let mut g = c.benchmark_group("real_by_sequence");

    // Per-entry decode cost, averaged over real entry sizes.
    g.bench_function("single", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let s = seqs[i % seqs.len()];
            i += 1;
            jmdict_core::lookup_by_sequence(black_box(s))
        })
    });

    // The Word List shape: resolve a whole batch of sequences at once.
    g.bench_function(format!("batch_{}", seqs.len()), |b| {
        b.iter(|| {
            seqs.iter()
                .map(|&s| jmdict_core::lookup_by_sequence(black_box(s)))
                .collect::<Vec<_>>()
        })
    });

    // End-to-end server/extension edge: resolve a batch *and* serialize it to
    // JSON. This is where zero-copy pays off — with rkyv we serialize straight
    // from the archived buffer, never materializing an owned WordEntry.
    g.bench_function(format!("batch_to_json_{}", seqs.len()), |b| {
        b.iter(|| {
            let resolved: Vec<_> = seqs
                .iter()
                .map(|&s| jmdict_core::lookup_by_sequence(black_box(s)))
                .collect();
            serde_json::to_string(&resolved).unwrap()
        })
    });

    g.finish();
}

criterion_group!(benches, bench_real_by_sequence);
criterion_main!(benches);
