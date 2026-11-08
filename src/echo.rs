//! Echo: line-level phonetic similarity, for verifying repeating forms.
//!
//! Villanelles, pantoums, duplexes and blues stanzas repeat lines — but
//! loosely: a word changes, a phrase is inserted, the line is cut short, or
//! its blocks are re-ordered. Echo flattens each
//! line to one phoneme sequence and aligns the two on the shared engine in
//! [`crate::align`], in a *similarity* profile: alike phonemes score
//! positive, unrelated ones negative, relative to zero points
//! ([`similarity`]). Alignment can then choose which regions to compare,
//! which rhyme's cost profile cannot.
//!
//! Two aligners implement [`EchoAligner`]:
//! - [`SemiGlobalAligner`], the default: one alignment in strict order with
//!   free ends.
//! - [`WatermanEggertAligner`], opt-in: disjoint local runs, assembled in or
//!   out of place, for echoes that keep only a fragment of their line or
//!   re-order it.
//!
//! A raw score is normalized by the lines' self-similarity, so a verbatim
//! repeat is 1 and unrelated lines sit near 0. Each aligner has a config
//! tuned on a labeled corpus ([`EchoConfig::for_aligner`]) and pass marks
//! for strict and loose forms ([`EchoStrictness::threshold`]); how they were
//! chosen, with the numbers, is in `data/echo/README.md`.
//!
//! Echo keeps its own coefficients ([`SimilarityConfig`]); it never reads
//! [`crate::RhymeDistanceConfig`].

pub mod alignment;
pub mod flatten;
pub mod scheme;
pub mod semi_global;
pub mod similarity;
pub mod template;
pub mod waterman_eggert;

pub use alignment::{
    AlignerKind, EchoAligner, EchoAlignment, EchoCosts, EchoSegment, EchoStep, Placement,
    RawAlignment, mark_in_place,
};
pub use flatten::{FlatLine, flatten, flatten_variants};
pub use scheme::{EchoFormResult, EchoPairResult, EchoPairScore, EchoStrictness, RepetitionScheme};
pub use semi_global::SemiGlobalAligner;
pub use similarity::{
    CALIBRATED_CONSONANT_ZERO_POINT, CALIBRATED_VOWEL_ZERO_POINT, SimilarityConfig, StressWeights,
    TUNED_CONSONANT_ZERO_POINT, ZeroPoints, calibrate_zero_points, similarity_table,
};
pub use template::EchoTemplate;
pub use waterman_eggert::WatermanEggertAligner;

use crate::align::{GapModel, ScoreTable};
use crate::consonant_distance::{AnalyticConsonantDistance, ConsonantDistanceScorer};
use crate::error::EchoError;
use crate::line::Line;
use crate::phoneme::Phoneme;
use crate::vowel_distance::{VowelDistanceScorer, VowelHexGraph};

/// Gap costs, in thousandths of a [unit](EchoDetector::unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapConfig {
    /// Opening a gap: an inserted or deleted phrase costs this once.
    pub gap_open: u32,
    /// Each gapped phoneme. 0 makes an insertion one event however long.
    pub gap_extend: u32,
    /// Opening a gap at a consonant that repeats its predecessor ("and
    /// dark" against "an dark").
    pub gap_open_repeated_consonant: u32,
}

impl Default for GapConfig {
    fn default() -> Self {
        Self {
            gap_open: 500,
            gap_extend: 0,
            gap_open_repeated_consonant: 250,
        }
    }
}

/// How Waterman–Eggert assembles local runs, in thousandths of a unit
/// (except `max_runs`). Semi-global ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureConfig {
    /// A moved block.
    pub move_cost: u32,
    /// Each local run after the first.
    pub run_open: u32,
    /// After the first, local runs scoring less end the search. ¾ unit: a
    /// lone primary vowel counts, a lone consonant doesn't.
    pub min_run: u32,
    /// At most this many local runs.
    pub max_runs: usize,
}

impl Default for StructureConfig {
    fn default() -> Self {
        Self {
            move_cost: 500,
            run_open: 500,
            min_run: 750,
            max_runs: 16,
        }
    }
}

/// What a raw score is divided by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EchoNormalization {
    /// The smaller self-similarity: how much of the shorter line is echoed.
    /// A truncation of a line is a full echo of it — but so is a short line
    /// found inside a long one, which is where many of the default's false
    /// positives come from.
    #[default]
    ShorterSelfSimilarity,
    /// The mean of both self-similarities: extra material on either side
    /// counts against the echo, so a truncation scores below 1. Fewer false
    /// positives from short lines; Waterman–Eggert's tuned config uses it.
    MeanSelfSimilarity,
}

/// Everything that shapes an echo score.
///
/// The default is semi-global's tuned config; see [`Self::for_aligner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoConfig {
    pub similarity: SimilarityConfig,
    pub gaps: GapConfig,
    pub structure: StructureConfig,
    pub normalization: EchoNormalization,
    /// The aligner [`EchoDetector::compare_lines`] and
    /// [`EchoDetector::compare_phonemes`] use.
    pub aligner: AlignerKind,
    /// A line with more pronunciation combinations than this is compared
    /// under its first pronunciations only.
    pub max_variants_per_line: usize,
    /// At most this many (variant, variant) pairs are compared per line pair.
    pub max_variant_pairs: usize,
}

impl Default for EchoConfig {
    fn default() -> Self {
        Self {
            similarity: SimilarityConfig::default(),
            gaps: GapConfig::default(),
            structure: StructureConfig::default(),
            normalization: EchoNormalization::default(),
            aligner: AlignerKind::default(),
            max_variants_per_line: 64,
            max_variant_pairs: 256,
        }
    }
}

impl EchoConfig {
    /// The config `aligner` was tuned with, by cross-validated search on the
    /// labeled corpus (`data/echo/README.md`). [`EchoStrictness::threshold`]
    /// was measured under these configs; another config needs its own
    /// thresholds.
    ///
    /// - Semi-global: the defaults, which are the hand-set structure plus
    ///   one tuned value, [`TUNED_CONSONANT_ZERO_POINT`]. The search's full
    ///   optimum was not significantly better and its vowel-side values
    ///   didn't agree across folds.
    /// - Waterman–Eggert: the search's optimum. It needs its structural
    ///   costs tuned (the zero point alone barely helps), and it uses mean
    ///   normalization. Several of these values sit near the edge of the
    ///   searched range (`run_open` at its maximum).
    pub fn for_aligner(aligner: AlignerKind) -> Self {
        match aligner {
            AlignerKind::SemiGlobal => Self::default(),
            AlignerKind::WatermanEggert => Self {
                similarity: SimilarityConfig {
                    vowel_coefficient: 6,
                    consonant_coefficient: 8,
                    vowel_zero_point: 261,
                    consonant_zero_point: TUNED_CONSONANT_ZERO_POINT,
                    stress_weights: StressWeights {
                        primary: 4,
                        secondary: 4,
                        unstressed: 3,
                    },
                },
                gaps: GapConfig {
                    gap_open: 615,
                    gap_extend: 116,
                    gap_open_repeated_consonant: 425,
                },
                structure: StructureConfig {
                    move_cost: 1277,
                    run_open: 1500,
                    min_run: 499,
                    max_runs: 16,
                },
                normalization: EchoNormalization::MeanSelfSimilarity,
                aligner: AlignerKind::WatermanEggert,
                ..Self::default()
            },
        }
    }
}

/// The best-scoring comparison of two lines over their pronunciation
/// variants.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoMatch {
    pub alignment: EchoAlignment,
    /// The variant of the first line that scored best.
    pub a: FlatLine,
    /// The variant of the second line that scored best.
    pub b: FlatLine,
    pub variant_pairs_tried: usize,
}

/// Scores echoes: a similarity table built from the phoneme engines, the
/// costs derived from an [`EchoConfig`], and the aligners.
///
/// Not generic over the engines — they are only consulted to build the
/// table — so one detector type serves any engine configuration.
#[derive(Debug, Clone)]
pub struct EchoDetector {
    table: ScoreTable,
    unit: i64,
    costs: EchoCosts,
    config: EchoConfig,
}

/// Scales a cost in thousandths of a unit to engine units, rounding.
fn scale(thousandths: u32, unit: i64) -> i64 {
    (i64::from(thousandths) * unit + 500).div_euclid(1000)
}

fn derive_costs(config: &EchoConfig, unit: i64) -> EchoCosts {
    let extend = scale(config.gaps.gap_extend, unit);
    EchoCosts {
        gaps: GapModel {
            open: scale(config.gaps.gap_open, unit),
            open_repeated_consonant: scale(config.gaps.gap_open_repeated_consonant, unit),
            extend_vowel: extend,
            extend_consonant: extend,
            extend_repeated_consonant: extend,
        },
        move_cost: scale(config.structure.move_cost, unit),
        run_open: scale(config.structure.run_open, unit),
        min_run: scale(config.structure.min_run, unit),
        max_runs: config.structure.max_runs,
    }
}

/// The aligner for `kind`.
pub fn aligner(kind: AlignerKind) -> &'static dyn EchoAligner {
    match kind {
        AlignerKind::SemiGlobal => &SemiGlobalAligner,
        AlignerKind::WatermanEggert => &WatermanEggertAligner,
    }
}

impl Default for EchoDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoDetector {
    /// The default engines and config (semi-global's tuned config).
    pub fn new() -> Self {
        Self::with_config(EchoConfig::default())
    }

    /// The default engines with `config`.
    pub fn with_config(config: EchoConfig) -> Self {
        Self::with_scorers_and_config(
            &VowelHexGraph::new(),
            &AnalyticConsonantDistance::new(),
            config,
        )
    }

    /// Builds the similarity table from `vowels` and `consonants`.
    pub fn with_scorers_and_config<V, C>(vowels: &V, consonants: &C, config: EchoConfig) -> Self
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        let (table, unit) = similarity_table(&config.similarity, vowels, consonants);
        Self {
            table,
            unit,
            costs: derive_costs(&config, unit),
            config,
        }
    }

    /// Rebuilds the detector from `vowels`, `consonants` and `config`.
    pub fn set_config<V, C>(&mut self, vowels: &V, consonants: &C, config: EchoConfig)
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        *self = Self::with_scorers_and_config(vowels, consonants, config);
    }

    pub fn config(&self) -> EchoConfig {
        self.config
    }

    /// The similarity of an identical primary-stressed vowel: the unit every
    /// echo cost is expressed in (thousandths of it, in [`EchoConfig`]).
    pub fn unit(&self) -> i64 {
        self.unit
    }

    pub fn table(&self) -> &ScoreTable {
        &self.table
    }

    /// The config's costs in engine units.
    pub fn costs(&self) -> &EchoCosts {
        &self.costs
    }

    /// A sequence aligned verbatim against itself: the sum of its phonemes'
    /// self-scores.
    pub fn self_similarity(&self, phonemes: &[Phoneme]) -> i64 {
        phonemes.iter().map(|p| self.table.score(p, p)).sum()
    }

    /// Compares two phoneme sequences with the configured aligner.
    pub fn compare_phonemes(&self, a: &[Phoneme], b: &[Phoneme]) -> EchoAlignment {
        self.compare_phonemes_with(self.config.aligner, a, b)
    }

    /// Compares two phoneme sequences with `kind`. An asymmetric aligner
    /// runs in both orientations; the better is reported in this one.
    pub fn compare_phonemes_with(
        &self,
        kind: AlignerKind,
        a: &[Phoneme],
        b: &[Phoneme],
    ) -> EchoAlignment {
        let raw = if a.is_empty() || b.is_empty() {
            RawAlignment::empty(a.len(), b.len())
        } else {
            let aligner = aligner(kind);
            let forward = aligner.align(a, b, &self.table, &self.costs);
            if aligner.is_symmetric() {
                forward
            } else {
                let backward = aligner.align(b, a, &self.table, &self.costs).transpose();
                if backward.raw > forward.raw {
                    backward
                } else {
                    forward
                }
            }
        };
        self.normalize(raw, a, b)
    }

    fn normalize(&self, raw: RawAlignment, a: &[Phoneme], b: &[Phoneme]) -> EchoAlignment {
        let (self_a, self_b) = (self.self_similarity(a), self.self_similarity(b));
        let denominator = match self.config.normalization {
            EchoNormalization::ShorterSelfSimilarity => self_a.min(self_b) as f64,
            EchoNormalization::MeanSelfSimilarity => (self_a + self_b) as f64 / 2.0,
        };
        let similarity = if denominator > 0.0 {
            (raw.raw as f64 / denominator) as f32
        } else {
            0.0
        };
        EchoAlignment {
            raw: raw.raw,
            similarity,
            self_similarity_a: self_a,
            self_similarity_b: self_b,
            normalization: self.config.normalization,
            steps: raw.steps,
            segments: raw.segments,
            a_coverage: raw.a_coverage,
            b_coverage: raw.b_coverage,
        }
    }

    /// Compares two lines with the configured aligner.
    pub fn compare_lines(&self, a: &Line, b: &Line) -> Result<EchoMatch, EchoError> {
        self.compare_lines_with(self.config.aligner, a, b)
    }

    /// Compares two lines with `kind`, over their pronunciation variants
    /// (capped per line and per pair), keeping the most similar. Ties keep
    /// the earliest pair, so first pronunciations win.
    pub fn compare_lines_with(
        &self,
        kind: AlignerKind,
        a: &Line,
        b: &Line,
    ) -> Result<EchoMatch, EchoError> {
        let mut a_variants = flatten_variants(a, self.config.max_variants_per_line)?;
        let mut b_variants = flatten_variants(b, self.config.max_variants_per_line)?;
        let cap = self.config.max_variant_pairs.max(1);
        while a_variants.len() * b_variants.len() > cap {
            // Drop to first pronunciations on the side with more variants.
            if a_variants.len() >= b_variants.len() {
                a_variants.truncate(1);
            } else {
                b_variants.truncate(1);
            }
        }
        let mut best: Option<(EchoAlignment, usize, usize)> = None;
        for (x, a_flat) in a_variants.iter().enumerate() {
            for (y, b_flat) in b_variants.iter().enumerate() {
                let alignment =
                    self.compare_phonemes_with(kind, &a_flat.phonemes, &b_flat.phonemes);
                if best
                    .as_ref()
                    .is_none_or(|(known, ..)| alignment.similarity > known.similarity)
                {
                    best = Some((alignment, x, y));
                }
            }
        }
        let (alignment, x, y) = best.expect("each line has at least one variant");
        Ok(EchoMatch {
            alignment,
            variant_pairs_tried: a_variants.len() * b_variants.len(),
            a: a_variants.swap_remove(x),
            b: b_variants.swap_remove(y),
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::OnceLock;

    use super::EchoDetector;
    use crate::CmuDictionary;
    use crate::phoneme::Phoneme;

    pub(crate) const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    pub(crate) fn dict() -> &'static CmuDictionary {
        static DICT: OnceLock<CmuDictionary> = OnceLock::new();
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    pub(crate) fn detector() -> &'static EchoDetector {
        static DETECTOR: OnceLock<EchoDetector> = OnceLock::new();
        DETECTOR.get_or_init(EchoDetector::new)
    }

    pub(crate) fn parse(s: &str) -> Vec<Phoneme> {
        s.split_whitespace().map(|t| t.parse().unwrap()).collect()
    }

    /// "dark demands I call my own" / "call my own dark demand sigh".
    pub(crate) const SWAP_A: &str = "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N";
    pub(crate) const SWAP_B: &str = "K AO1 L M AY1 OW1 N D AA1 R K D IH0 M AE1 N D S AY1";

    /// Three blocks, X Y Z ("golden apples", "under the moon", "shining"),
    /// chosen so no block's edge resembles another's: greedy local runs
    /// (Waterman–Eggert) overextend across junctions where neighbors happen to
    /// be alike, which would blur what these fixtures test.
    pub(crate) const BLOCK_X: &str = "G OW1 L D AH0 N AE1 P AH0 L Z";
    pub(crate) const BLOCK_Y: &str = "AH1 N D ER0 DH AH0 M UW1 N";
    pub(crate) const BLOCK_Z: &str = "SH AY1 N IH0 NG";

    /// "I saw a bird in the forest" / "I saw a bird it was in the forest".
    pub(crate) const BIRD: &str = "AY1 S AO1 AH0 B ER1 D IH0 N DH AH0 F AO1 R AH0 S T";
    pub(crate) const BIRD_INSERTED: &str =
        "AY1 S AO1 AH0 B ER1 D IH1 T W AA1 Z IH0 N DH AH0 F AO1 R AH0 S T";

    pub(crate) fn sim(a: &str, b: &str) -> i64 {
        detector()
            .table()
            .score(&a.parse().unwrap(), &b.parse().unwrap())
    }

    pub(crate) fn self_sim(s: &str) -> i64 {
        detector().self_similarity(&parse(s))
    }

    pub(crate) fn costs() -> &'static super::EchoCosts {
        detector().costs()
    }

    /// Runs `aligner` directly (no orientation choice, no normalization).
    pub(crate) fn raw(aligner: &dyn super::EchoAligner, a: &str, b: &str) -> super::RawAlignment {
        aligner.align(&parse(a), &parse(b), detector().table(), costs())
    }

    /// "the cat sat on the mat" / "brisk winds buffet quarries".
    pub(crate) const UNRELATED_A: &str = "DH AH0 K AE1 T S AE1 T AA1 N DH AH0 M AE1 T";
    pub(crate) const UNRELATED_B: &str = "B R IH1 S K W IH1 N D Z B AH1 F AH0 T K W AO1 R IY0 Z";

    /// xorshift64.
    pub(crate) struct Rng(pub(crate) u64);

    impl Rng {
        pub(crate) fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }

        pub(crate) fn sequence(&mut self, alphabet: &[Phoneme], max_len: usize) -> Vec<Phoneme> {
            (0..self.below(max_len + 1))
                .map(|_| alphabet[self.below(alphabet.len())].clone())
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    /// Every structural promise an [`EchoAlignment`] makes.
    fn assert_invariants(
        alignment: &EchoAlignment,
        a: &[Phoneme],
        b: &[Phoneme],
        kind: AlignerKind,
    ) {
        let context = || format!("{kind} on {a:?} vs {b:?}: {alignment:?}");
        // Steps are exactly the segments' steps, segments in a order.
        let mut next = 0;
        for segment in &alignment.segments {
            assert_eq!(segment.steps.start, next, "{}", context());
            next = segment.steps.end;
            let steps = &alignment.steps[segment.steps.clone()];
            assert_eq!(
                segment.score,
                steps.iter().map(|s| s.score).sum::<i64>(),
                "{}",
                context()
            );
            for step in steps {
                if let Some(i) = step.a_index {
                    assert!(segment.a_range.contains(&i), "{}", context());
                    assert_eq!(step.step.a(), Some(&a[i]));
                }
                if let Some(j) = step.b_index {
                    assert!(segment.b_range.contains(&j), "{}", context());
                    assert_eq!(step.step.b(), Some(&b[j]));
                }
            }
        }
        assert_eq!(next, alignment.steps.len(), "{}", context());
        assert!(
            alignment
                .segments
                .windows(2)
                .all(|w| w[0].a_range.start <= w[1].a_range.start),
            "{}",
            context()
        );
        // raw = Σ score − Σ open.
        let assembled: i64 = alignment
            .segments
            .iter()
            .map(|s| s.score - s.open_cost)
            .sum();
        assert_eq!(alignment.raw, assembled, "{}", context());
        // Coverage counts pairs, at most one per phoneme.
        let mut a_coverage = vec![0u8; a.len()];
        let mut b_coverage = vec![0u8; b.len()];
        for step in &alignment.steps {
            if let (Some(i), Some(j)) = (step.a_index, step.b_index) {
                a_coverage[i] += 1;
                b_coverage[j] += 1;
            }
        }
        assert_eq!(alignment.a_coverage, a_coverage, "{}", context());
        assert_eq!(alignment.b_coverage, b_coverage, "{}", context());
        assert!(
            a_coverage.iter().chain(&b_coverage).all(|&c| c <= 1),
            "{}",
            context()
        );
        // Normalization.
        let denominator = alignment.self_similarity_a.min(alignment.self_similarity_b);
        if denominator > 0 {
            let expected = alignment.raw as f64 / denominator as f64;
            assert!(
                (f64::from(alignment.similarity) - expected).abs() < 1e-5,
                "{}",
                context()
            );
        } else {
            assert_eq!(alignment.similarity, 0.0);
        }
        // Nothing beats a verbatim repeat.
        assert!(alignment.similarity <= 1.0 + 1e-6, "{}", context());
    }

    #[test]
    fn invariants_hold_on_random_sweeps_for_every_aligner() {
        let alphabet = parse("AE1 AE0 AE2 IH0 OW1 ER0 AY1 K T T S Z M N D L");
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let d = detector();
        for _ in 0..800 {
            let (a, b) = (rng.sequence(&alphabet, 18), rng.sequence(&alphabet, 18));
            for kind in AlignerKind::ALL {
                let forward = d.compare_phonemes_with(kind, &a, &b);
                assert_invariants(&forward, &a, &b, kind);
                let backward = d.compare_phonemes_with(kind, &b, &a);
                assert_eq!(
                    forward.raw, backward.raw,
                    "{kind} must be symmetric: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn verbatim_repeats_score_exactly_one() {
        for kind in AlignerKind::ALL {
            for line in [SWAP_A, BIRD, UNRELATED_B, "AH0"] {
                let a = parse(line);
                assert_eq!(
                    detector().compare_phonemes_with(kind, &a, &a).similarity,
                    1.0,
                    "{kind} {line}"
                );
            }
        }
    }

    #[test]
    fn empty_sequences_score_zero() {
        for kind in AlignerKind::ALL {
            let alignment = detector().compare_phonemes_with(kind, &[], &parse(SWAP_A));
            assert_eq!((alignment.raw, alignment.similarity), (0, 0.0));
            assert!(alignment.steps.is_empty());
            assert_eq!(alignment.b_coverage.len(), parse(SWAP_A).len());
        }
    }

    #[test]
    fn normalization_chooses_the_denominator() {
        let (dark, darker) = (parse("D AA1 R K"), parse("D AA1 R K ER0"));
        let shorter = detector().compare_phonemes(&dark, &darker);
        assert_eq!(shorter.similarity, 1.0);
        let config = EchoConfig {
            normalization: EchoNormalization::MeanSelfSimilarity,
            ..EchoConfig::default()
        };
        let mean = EchoDetector::with_config(config).compare_phonemes(&dark, &darker);
        assert_eq!(mean.raw, shorter.raw);
        let expected = shorter.raw as f64
            / ((shorter.self_similarity_a + shorter.self_similarity_b) as f64 / 2.0);
        assert!((f64::from(mean.similarity) - expected).abs() < 1e-6);
        assert!(mean.similarity < 1.0);
    }

    #[test]
    fn config_costs_scale_with_the_unit() {
        let d = detector();
        assert_eq!(d.unit(), 3 * i64::from(CALIBRATED_VOWEL_ZERO_POINT) * 4);
        assert_eq!(d.costs().gaps.open * 2, d.unit());
        assert_eq!(d.costs().gaps.extend_vowel, 0);
        assert_eq!(d.costs().min_run * 4, d.unit() * 3);
        let mut config = EchoConfig::default();
        config.gaps.gap_open = 250;
        let mut cheaper = EchoDetector::new();
        cheaper.set_config(
            &VowelHexGraph::new(),
            &AnalyticConsonantDistance::new(),
            config,
        );
        assert_eq!(cheaper.costs().gaps.open * 4, cheaper.unit());
        assert_eq!(cheaper.config(), config);
    }

    #[test]
    fn lines_compare_over_pronunciation_variants() {
        let fire_truck = Line::new("fire truck", dict());
        let echo = detector().compare_lines(&fire_truck, &fire_truck).unwrap();
        assert_eq!(echo.alignment.similarity, 1.0);
        assert_eq!(echo.variant_pairs_tried, 4);
        assert_eq!(echo.a.word_count(), 2);

        // Resegmentation is invisible: the flattened phonemes are compared.
        let a = Line::new("dark demands I call my own", dict());
        let b = Line::new("call my own dark demand sigh", dict());
        let echo = detector()
            .compare_lines_with(AlignerKind::WatermanEggert, &a, &b)
            .unwrap();
        assert!(
            echo.alignment.similarity > 0.85,
            "{}",
            echo.alignment.similarity
        );
        let strict = detector()
            .compare_lines_with(AlignerKind::SemiGlobal, &a, &b)
            .unwrap();
        assert!(strict.alignment.similarity < 0.7);
    }

    #[test]
    fn variant_pairs_are_capped() {
        let config = EchoConfig {
            max_variant_pairs: 2,
            ..EchoConfig::default()
        };
        let capped = EchoDetector::with_config(config);
        let fire_truck = Line::new("fire truck", dict());
        let echo = capped.compare_lines(&fire_truck, &fire_truck).unwrap();
        assert_eq!(echo.variant_pairs_tried, 2);
        assert_eq!(echo.alignment.similarity, 1.0);
    }

    #[test]
    fn unknown_words_are_errors() {
        let a = Line::new("call my own", dict());
        let b = Line::new("call my xyzzy", dict());
        assert_eq!(
            detector().compare_lines(&a, &b),
            Err(EchoError::UnknownWord(crate::UnknownWordError {
                word: "xyzzy".to_string()
            }))
        );
    }
}
