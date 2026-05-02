use criterion::{black_box, criterion_group, criterion_main, Criterion};
use japanese_utils::extract_japanese_run;

fn bench_extract(c: &mut Criterion) {
    let mut g = c.benchmark_group("extract_japanese_run");

    g.bench_function("run_from_start", |b| {
        b.iter(|| extract_japanese_run(black_box("日本語テキスト"), black_box(0)))
    });

    g.bench_function("run_from_mid", |b| {
        // offset 2 in "飲み込む" → returns "込む"
        b.iter(|| extract_japanese_run(black_box("飲み込む"), black_box(2)))
    });

    g.bench_function("no_japanese_at_offset", |b| {
        b.iter(|| extract_japanese_run(black_box("hello日本語"), black_box(0)))
    });

    g.bench_function("long_run", |b| {
        b.iter(|| extract_japanese_run(black_box("私は毎日日本語を勉強しています"), black_box(0)))
    });

    g.finish();
}

criterion_group!(benches, bench_extract);
criterion_main!(benches);
