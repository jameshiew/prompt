use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use prompt::tokenizer::tokenize;

fn criterion_config() -> Criterion {
    Criterion::default().sample_size(50)
}

fn criterion_benchmark(c: &mut Criterion) {
    let text = "a ".repeat(100_000);

    c.bench_function("tokenize", |b| b.iter(|| tokenize(black_box(&text))));
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = criterion_benchmark
}
criterion_main!(benches);
