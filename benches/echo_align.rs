//! Times each echo aligner (under its tuned config) on representative line pairs (phoneme level, no
//! dictionary): a verbatim repeat, an insertion, a two-block swap, and an
//! unrelated pair. Run with `cargo bench --bench echo_align`.

#[cfg(not(target_arch = "wasm32"))]
use std::{hint::black_box, time::Duration};

#[cfg(not(target_arch = "wasm32"))]
use criterion::{Criterion, criterion_group, criterion_main};
#[cfg(not(target_arch = "wasm32"))]
use mime_reader::{
    Phoneme,
    echo::{AlignerKind, EchoConfig, EchoDetector},
};

#[cfg(not(target_arch = "wasm32"))]
fn ph(s: &str) -> Vec<Phoneme> {
    s.split_whitespace()
        .map(|p| p.parse().expect("valid ARPAbet"))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn bench_echo_align(c: &mut Criterion) {
    let cases = [
        // "dark demands I call my own", against itself.
        (
            "verbatim_19ph",
            "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N",
            "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N",
        ),
        // "I saw a bird in the forest" / "... bird it was in the forest".
        (
            "insertion_17_22ph",
            "AY1 S AO1 AH0 B ER1 D IH0 N DH AH0 F AO1 R AH0 S T",
            "AY1 S AO1 AH0 B ER1 D IH1 T W AA1 Z IH0 N DH AH0 F AO1 R AH0 S T",
        ),
        // "dark demands I call my own" / "call my own dark demand sigh".
        (
            "swap_19ph",
            "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N",
            "K AO1 L M AY1 OW1 N D AA1 R K D IH0 M AE1 N D S AY1",
        ),
        // "the cat sat on the mat" / "brisk winds buffet quarries".
        (
            "unrelated_15_21ph",
            "DH AH0 K AE1 T S AE1 T AA1 N DH AH0 M AE1 T",
            "B R IH1 S K W IH1 N D Z B AH1 F AH0 T K W AO1 R IY0 Z",
        ),
    ];

    let mut group = c.benchmark_group("echo");
    group
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    for (name, a, b) in cases {
        let (a, b) = (ph(a), ph(b));
        for kind in AlignerKind::ALL {
            // Each aligner under its tuned config, as it would be used.
            let detector = EchoDetector::with_config(EchoConfig::for_aligner(kind));
            group.bench_function(format!("{kind}/{name}"), |bench| {
                bench.iter(|| detector.compare_phonemes(black_box(&a), black_box(&b)))
            });
        }
    }
    group.finish();
}

#[cfg(not(target_arch = "wasm32"))]
criterion_group!(benches, bench_echo_align);
#[cfg(not(target_arch = "wasm32"))]
criterion_main!(benches);

#[cfg(target_arch = "wasm32")]
fn main() {}
