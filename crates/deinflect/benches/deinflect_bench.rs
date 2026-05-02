use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_deinflect(c: &mut Criterion) {
    let mut g = c.benchmark_group("deinflect");

    g.bench_function("plain_form", |b| {
        b.iter(|| deinflect::deinflect(black_box("食べる")))
    });

    g.bench_function("chained_passive_negative", |b| {
        // 食べられなかった → 食べられる → 食べる (depth 2)
        b.iter(|| deinflect::deinflect(black_box("食べられなかった")))
    });

    g.bench_function("godan_te_progressive_negative", |b| {
        // 飲んでいなかった — godan + te-form + progressive + negative past
        b.iter(|| deinflect::deinflect(black_box("飲んでいなかった")))
    });

    g.bench_function("no_match", |b| {
        b.iter(|| deinflect::deinflect(black_box("hello")))
    });

    g.finish();
}

criterion_group!(benches, bench_deinflect);
criterion_main!(benches);
