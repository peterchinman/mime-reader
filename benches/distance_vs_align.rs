//! Compares `RhymeDistanceScorer::distance` (score only, no edit recording)
//! against `align` (score + traceback + owned step list) on representative
//! rhyming-part sizes. Run with `cargo bench`.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use mime_reader::{Phoneme, RhymeDistanceScorer};

fn ph(s: &str) -> Vec<Phoneme> {
    s.split_whitespace()
        .map(|p| p.parse().expect("valid ARPAbet"))
        .collect()
}

fn bench_distance_vs_align(c: &mut Criterion) {
    let scorer = RhymeDistanceScorer::new();

    // (name, leader-side, target-side) — spanning the realistic range:
    // rhyming parts are usually 1-3 syllables, a handful of phonemes.
    let cases = [
        // Identical: prefix/suffix stripping reduces the DP to nothing.
        ("identical_2ph", "AE1 T", "AE1 T"),
        // cat/cot: one substitution after stripping.
        ("vowel_sub_2ph", "AE1 T", "AA1 T"),
        // most/moats: transposition, the most complex DP transition.
        ("transposition_4ph", "M OW1 S T", "M OW1 T S"),
        // Multisyllabic with several edits — worst realistic case.
        (
            "long_9ph",
            "AO2 R N Y AH0 K AE1 L AH0",
            "EY1 N Y AH0 P EH2 N S AH0",
        ),
    ];

    let mut group = c.benchmark_group("rhyme");
    group
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));

    for (name, a, b) in cases {
        let a = ph(a);
        let b = ph(b);
        group.bench_function(format!("distance/{name}"), |bench| {
            bench.iter(|| scorer.distance(black_box(&a), black_box(&b)))
        });
        group.bench_function(format!("align/{name}"), |bench| {
            bench.iter(|| scorer.align(black_box(&a), black_box(&b)))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_distance_vs_align);
criterion_main!(benches);
