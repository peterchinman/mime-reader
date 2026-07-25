//! Full-dictionary `Analyzer::find_rhymes` scans across representative
//! targets. This is the tripwire for scan-cost regressions and the yardstick
//! for proposed scan optimizations (syllable prefilters, DP scratch reuse,
//! early-abandon, ...). Run with `cargo bench --bench find_rhymes`.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use mime_reader::{Analyzer, CmuDictionary, Compare, RhymeDistanceScorer, RhymeNormalization};

const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

fn bench_find_rhymes(c: &mut Criterion) {
    // Built once, outside all timing: the scan cost is what's under test,
    // not the 3.8 MB dictionary parse.
    let dict = CmuDictionary::load(DICT_PATH).expect("failed to load dictionary");
    let analyzer = Analyzer::new(dict, RhymeDistanceScorer::new());

    let mut group = c.benchmark_group("find_rhymes");
    // Each iteration is a full ~134k-word scan (~ms, not ns), so fewer,
    // longer samples than criterion's defaults.
    group
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(11));

    // Rhyming-part scans: the common interactive case. Syllable count of the
    // target's rhyming part drives how much of the dictionary is scored
    // versus rejected as too short.
    let rhyming_part_cases = [
        ("1syl_cat", "cat"),           // AE1 T
        ("2syl_hollow", "hollow"),     // AA1 L OW0
        ("3syl_calliope", "calliope"), // AY1 AH0 P IY2
    ];
    for (name, target) in rhyming_part_cases {
        group.bench_function(name, |bench| {
            bench.iter(|| {
                analyzer
                    .find_rhymes(
                        black_box(target),
                        0.0..=1.0,
                        RhymeNormalization::PerMeanPhoneme,
                        Compare::RhymingPart,
                    )
                    .unwrap()
            })
        });
    }

    // Whole-word comparison scores every entry against the full target
    // pronunciation — no too-short rejection at all.
    group.bench_function("whole_word_cat", |bench| {
        bench.iter(|| {
            analyzer
                .find_rhymes(
                    black_box("cat"),
                    0.0..=1.0,
                    RhymeNormalization::PerMeanPhoneme,
                    Compare::WholeWord,
                )
                .unwrap()
        })
    });

    // Full range: every scored word is retained and sorted — the
    // "whole dictionary ranked by rhyme distance" extreme.
    group.bench_function("ranked_full_dict_cat", |bench| {
        bench.iter(|| {
            analyzer
                .find_rhymes(
                    black_box("cat"),
                    0.0..=f32::INFINITY,
                    RhymeNormalization::PerMeanPhoneme,
                    Compare::RhymingPart,
                )
                .unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, bench_find_rhymes);
criterion_main!(benches);
