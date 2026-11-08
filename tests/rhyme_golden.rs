//! Golden rhyme alignments: the rhyme DP was ported onto the shared
//! alignment engine (`src/align`) behavior-preserving — same distances and
//! the same steps, including how ties are broken. Each line of
//! `data/rhyme_golden.jsonl` is one pair aligned by the original DP (before
//! the port), under one of four configs chosen to force ties: every
//! tie-break priority occurs on some golden path. The test checks the
//! current engine reproduces all of them.
//!
//! If the tie-breaking is changed on purpose, regenerate the file from the
//! new engine and say so in the commit.
#![cfg(not(target_arch = "wasm32"))] // serde_json is a native-only dev-dependency

use std::collections::BTreeMap;

use mime_reader::{
    AlignmentStep, AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, Phoneme,
    RhymeDistanceConfig, RhymeDistanceScorer, TranspositionDirection, VowelHexGraph,
    VowelHexGraphConfig,
};

/// The phoneme engines every golden config uses: the engine defaults at
/// b2cc446 (identical at 7dc354f), written out.
fn golden_vowel_config() -> VowelHexGraphConfig {
    VowelHexGraphConfig {
        vowel_stress_penalty: 1,
        vowel_distance_coefficient: 5,
        er_ah_distance: 3,
    }
}

fn golden_consonant_config() -> AnalyticConsonantDistanceConfig {
    AnalyticConsonantDistanceConfig {
        place_distance_coefficient: 1,
        unrelated_penalty: 10,
        voiced_penalty: 1,
        sibilance_penalty: 1,
        r_l_distance: 1,
        w_v_distance: 2,
        affricate_sibilant_fricative_penalty: 1,
        affricate_plosive_penalty: 2,
        affricate_non_sibilant_fricative_penalty: 2,
    }
}

/// The rhyme config named in a golden line's `"config"` field.
fn golden_config(name: &str) -> RhymeDistanceConfig {
    match name {
        // `RhymeDistanceConfig::default()` at b2cc446, written out.
        "default" => RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 100_000,
            vowel_coefficient: 7,
            consonant_coefficient: 1,
            vowel_indel_penalty: 100_000,
            consonant_indel_penalty: 500,
            consonant_repeated_penalty: 100,
            transposition_cost: 200,
        },
        // Every indel, the repeat discount and a cross-class pair cost the
        // same, and a swap costs exactly two indels: gap-vs-gap, gap-vs-pair
        // and swap-vs-(gap, match, gap) ties everywhere.
        "all_equal" => RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 500,
            vowel_coefficient: 1,
            consonant_coefficient: 1,
            vowel_indel_penalty: 500,
            consonant_indel_penalty: 500,
            consonant_repeated_penalty: 500,
            transposition_cost: 1000,
        },
        // The default with cheap consonant gaps, free repeated consonants and
        // a swap costing two consonant gaps.
        "cheap_gaps" => RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 100_000,
            vowel_coefficient: 7,
            consonant_coefficient: 1,
            vowel_indel_penalty: 100_000,
            consonant_indel_penalty: 200,
            consonant_repeated_penalty: 0,
            transposition_cost: 400,
        },
        // Everything free: every alignment scores 0, so the steps are decided
        // by the tie priorities alone.
        "all_zero" => RhymeDistanceConfig {
            vowel_to_consonant_mismatch: 0,
            vowel_coefficient: 0,
            consonant_coefficient: 0,
            vowel_indel_penalty: 0,
            consonant_indel_penalty: 0,
            consonant_repeated_penalty: 0,
            transposition_cost: 0,
        },
        other => panic!("unknown golden config {other:?}"),
    }
}

/// The scorer for a golden config: the pinned engines plus the rhyme config.
fn golden_scorer(name: &str) -> RhymeDistanceScorer {
    RhymeDistanceScorer::with_scorers_and_config(
        VowelHexGraph::with_config(golden_vowel_config()),
        AnalyticConsonantDistance::with_config(golden_consonant_config()),
        golden_config(name),
    )
}

fn parse(s: &str) -> Vec<Phoneme> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

/// A step as the golden file writes it: `<kind> <a or -> <b or ->`.
fn step_string(step: &AlignmentStep<'_>) -> String {
    match *step {
        AlignmentStep::Match { a, b } => format!("match {a} {b}"),
        AlignmentStep::Substitution { a, b } => format!("sub {a} {b}"),
        AlignmentStep::Indel {
            a: Some(a),
            b: None,
        } => format!("indel {a} -"),
        AlignmentStep::Indel {
            a: None,
            b: Some(b),
        } => format!("indel - {b}"),
        AlignmentStep::Indel { .. } => panic!("an indel has exactly one side"),
        AlignmentStep::Transposition {
            a,
            b,
            direction: TranspositionDirection::Right,
        } => format!("right {a} {b}"),
        AlignmentStep::Transposition {
            a,
            b,
            direction: TranspositionDirection::Left,
        } => format!("left {a} {b}"),
    }
}

#[test]
fn the_ported_rhyme_dp_reproduces_the_original_alignments() {
    // The goldens pin every config value, so retuning a default doesn't
    // silently change what they test; these say when the defaults moved.
    assert_eq!(golden_vowel_config(), VowelHexGraphConfig::default());
    assert_eq!(
        golden_consonant_config(),
        AnalyticConsonantDistanceConfig::default()
    );
    assert_eq!(golden_config("default"), RhymeDistanceConfig::default());

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/rhyme_golden.jsonl");
    let text = std::fs::read_to_string(path).expect("the golden file is readable");
    let mut scorers: BTreeMap<String, RhymeDistanceScorer> = BTreeMap::new();
    let mut failures = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let case: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
        let config = case["config"].as_str().expect("config");
        let scorer = scorers
            .entry(config.to_string())
            .or_insert_with(|| golden_scorer(config));
        let a = parse(case["a"].as_str().expect("a"));
        let b = parse(case["b"].as_str().expect("b"));
        let distance = case["distance"].as_u64().expect("distance") as u32;
        let steps: Vec<&str> = case["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .map(|s| s.as_str().expect("a step"))
            .collect();
        let output = scorer.align(&a, &b);
        let got: Vec<String> = output.alignment().iter().map(step_string).collect();
        if scorer.distance(&a, &b) != distance || output.score() != distance || got != steps {
            failures.push(format!(
                "line {}: {line}\n  got {} {got:?}",
                n + 1,
                output.score()
            ));
        }
    }
    assert!(text.lines().count() >= 400);
    assert_eq!(scorers.len(), 4);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
