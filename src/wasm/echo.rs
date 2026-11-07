//! Echo bindings: line-pair scoring and form checks on the analyzer.
//!
//! The detector is rebuilt per call from the analyzer's current vowel and
//! consonant configs plus its echo config (building the similarity table
//! is a few thousand engine calls — microseconds), so it can never drift
//! from the engines the tuning page is editing.

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use super::{AlignmentStepDto, WasmAnalyzer};
use crate::echo::{
    AlignerKind, EchoConfig, EchoDetector, EchoMatch, EchoNormalization, EchoTemplate, GapConfig,
    Placement, RepetitionScheme, SimilarityConfig, StressWeights, StructureConfig,
};
use crate::line::Line;
use crate::{AnalyticConsonantDistance, VowelHexGraph};

#[wasm_bindgen(js_name = defaultEchoConfig)]
pub fn default_echo_config() -> EchoConfigDto {
    EchoConfig::default().into()
}

#[wasm_bindgen]
impl WasmAnalyzer {
    /// The current echo config.
    #[wasm_bindgen(js_name = echoConfig)]
    pub fn echo_config(&self) -> EchoConfigDto {
        self.echo_config.into()
    }

    /// Replaces the echo config used by `scoreEcho` and `checkEchoForm`.
    #[wasm_bindgen(js_name = setEchoConfig)]
    pub fn set_echo_config(&mut self, config: EchoConfigDto) {
        self.echo_config = config.into();
    }

    /// Scores how strongly line `b` echoes line `a`, with `aligner` (the
    /// config's aligner when omitted).
    #[wasm_bindgen(js_name = scoreEcho)]
    pub fn score_echo(
        &self,
        a: &str,
        b: &str,
        aligner: Option<AlignerKindDto>,
    ) -> Result<EchoAlignmentDto, JsError> {
        let detector = self.echo_detector();
        let kind = aligner.map_or(self.echo_config.aligner, AlignerKind::from);
        let (a_line, b_line) = (Line::new(a, &*self.dict), Line::new(b, &*self.dict));
        let echo = detector.compare_lines_with(kind, &a_line, &b_line)?;
        Ok(EchoAlignmentDto::new(&echo, &a_line, &b_line, kind))
    }

    /// Checks a poem's lines against a repetition template. Blank lines
    /// (stanza breaks) are skipped, so indices count non-blank lines.
    #[wasm_bindgen(js_name = checkEchoForm)]
    pub fn check_echo_form(
        &self,
        lines: Vec<String>,
        template: EchoTemplateDto,
        threshold: f32,
        aligner: Option<AlignerKindDto>,
    ) -> Result<EchoFormResultDto, JsError> {
        let template = EchoTemplate::try_from(template)?;
        let expected_line_count = template.expected_line_count;
        let kind = aligner.map_or(self.echo_config.aligner, AlignerKind::from);
        let scheme = RepetitionScheme::new(template, threshold, kind);
        let lines: Vec<Line> = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Line::new(line, &*self.dict))
            .collect();
        let result = scheme.check(&lines, &self.echo_detector());
        Ok(EchoFormResultDto {
            pairs: result
                .pairs
                .into_iter()
                .map(|pair| EchoPairResultDto {
                    leader: pair.leader,
                    follower: pair.follower,
                    outcome: match pair.outcome {
                        Ok(score) => EchoPairOutcomeDto::Scored {
                            raw: score.alignment.alignment.raw,
                            similarity: score.alignment.alignment.similarity,
                            distance: score.alignment.alignment.distance(),
                            passed: score.passed,
                        },
                        Err(error) => EchoPairOutcomeDto::Error {
                            message: error.to_string(),
                        },
                    },
                })
                .collect(),
            missing_pairs: result
                .missing_pairs
                .into_iter()
                .map(|(a, b)| [a, b])
                .collect(),
            line_count: lines.len(),
            expected_line_count,
            line_count_matches: result.line_count_matches,
            passed: result.passed,
        })
    }
}

impl WasmAnalyzer {
    fn echo_detector(&self) -> EchoDetector {
        EchoDetector::with_scorers_and_config(
            &VowelHexGraph::with_config(self.scorer.vowel_config()),
            &AnalyticConsonantDistance::with_config(self.scorer.consonant_config()),
            self.echo_config,
        )
    }
}

// ---- DTOs -------------------------------------------------------------------

/// Which echo aligner to use. Mirrors [`AlignerKind`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum AlignerKindDto {
    SemiGlobal,
    WatermanEggert,
    Jump,
}

impl From<AlignerKindDto> for AlignerKind {
    fn from(kind: AlignerKindDto) -> Self {
        match kind {
            AlignerKindDto::SemiGlobal => Self::SemiGlobal,
            AlignerKindDto::WatermanEggert => Self::WatermanEggert,
            AlignerKindDto::Jump => Self::Jump,
        }
    }
}

impl From<AlignerKind> for AlignerKindDto {
    fn from(kind: AlignerKind) -> Self {
        match kind {
            AlignerKind::SemiGlobal => Self::SemiGlobal,
            AlignerKind::WatermanEggert => Self::WatermanEggert,
            AlignerKind::Jump => Self::Jump,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum EchoNormalizationDto {
    ShorterSelfSimilarity,
    MeanSelfSimilarity,
}

/// The echo config, flattened. Costs are in thousandths of a unit (the
/// similarity of an identical primary-stressed vowel).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct EchoConfigDto {
    vowel_coefficient: u32,
    consonant_coefficient: u32,
    vowel_zero_point: u32,
    consonant_zero_point: u32,
    primary_weight: u32,
    secondary_weight: u32,
    unstressed_weight: u32,
    gap_open: u32,
    gap_extend: u32,
    gap_open_repeated_consonant: u32,
    move_cost: u32,
    run_open: u32,
    min_run: u32,
    reuse_penalty: u32,
    max_runs: u32,
    normalization: EchoNormalizationDto,
    aligner: AlignerKindDto,
    max_variants_per_line: u32,
    max_variant_pairs: u32,
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<EchoConfig> for EchoConfigDto {
    fn from(c: EchoConfig) -> Self {
        let s = c.similarity;
        Self {
            vowel_coefficient: s.vowel_coefficient,
            consonant_coefficient: s.consonant_coefficient,
            vowel_zero_point: s.vowel_zero_point,
            consonant_zero_point: s.consonant_zero_point,
            primary_weight: s.stress_weights.primary,
            secondary_weight: s.stress_weights.secondary,
            unstressed_weight: s.stress_weights.unstressed,
            gap_open: c.gaps.gap_open,
            gap_extend: c.gaps.gap_extend,
            gap_open_repeated_consonant: c.gaps.gap_open_repeated_consonant,
            move_cost: c.structure.move_cost,
            run_open: c.structure.run_open,
            min_run: c.structure.min_run,
            reuse_penalty: c.structure.reuse_penalty,
            max_runs: to_u32(c.structure.max_runs),
            normalization: match c.normalization {
                EchoNormalization::ShorterSelfSimilarity => {
                    EchoNormalizationDto::ShorterSelfSimilarity
                }
                EchoNormalization::MeanSelfSimilarity => EchoNormalizationDto::MeanSelfSimilarity,
            },
            aligner: c.aligner.into(),
            max_variants_per_line: to_u32(c.max_variants_per_line),
            max_variant_pairs: to_u32(c.max_variant_pairs),
        }
    }
}

impl From<EchoConfigDto> for EchoConfig {
    fn from(c: EchoConfigDto) -> Self {
        Self {
            similarity: SimilarityConfig {
                vowel_coefficient: c.vowel_coefficient,
                consonant_coefficient: c.consonant_coefficient,
                vowel_zero_point: c.vowel_zero_point,
                consonant_zero_point: c.consonant_zero_point,
                stress_weights: StressWeights {
                    primary: c.primary_weight,
                    secondary: c.secondary_weight,
                    unstressed: c.unstressed_weight,
                },
            },
            gaps: GapConfig {
                gap_open: c.gap_open,
                gap_extend: c.gap_extend,
                gap_open_repeated_consonant: c.gap_open_repeated_consonant,
            },
            structure: StructureConfig {
                move_cost: c.move_cost,
                run_open: c.run_open,
                min_run: c.min_run,
                reuse_penalty: c.reuse_penalty,
                max_runs: c.max_runs as usize,
            },
            normalization: match c.normalization {
                EchoNormalizationDto::ShorterSelfSimilarity => {
                    EchoNormalization::ShorterSelfSimilarity
                }
                EchoNormalizationDto::MeanSelfSimilarity => EchoNormalization::MeanSelfSimilarity,
            },
            aligner: c.aligner.into(),
            max_variants_per_line: c.max_variants_per_line as usize,
            max_variant_pairs: c.max_variant_pairs as usize,
        }
    }
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum PlacementDto {
    InPlace,
    Moved,
}

/// One aligned step: which phonemes it consumed (indices into the
/// `*_phonemes` arrays), what it scored, and which segment it belongs to.
#[derive(Serialize, Debug, Tsify)]
pub struct EchoStepDto {
    step: AlignmentStepDto,
    score: i64,
    a_index: Option<usize>,
    b_index: Option<usize>,
    segment: usize,
}

/// A contiguous aligned stretch; ranges are half-open phoneme indices.
#[derive(Serialize, Debug, Tsify)]
pub struct EchoSegmentDto {
    a_start: usize,
    a_end: usize,
    b_start: usize,
    b_end: usize,
    score: i64,
    open_cost: i64,
    placement: PlacementDto,
}

/// A scored echo comparison, with everything needed to draw it: both lines'
/// words and phonemes (under the pronunciation variants that scored best),
/// where each word starts, the steps, segments and per-phoneme coverage
/// (0 = unaligned, 2+ = reused).
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EchoAlignmentDto {
    aligner: AlignerKindDto,
    raw: i64,
    self_a: i64,
    self_b: i64,
    similarity: f32,
    distance: f32,
    a_words: Vec<String>,
    b_words: Vec<String>,
    a_phonemes: Vec<String>,
    b_phonemes: Vec<String>,
    a_word_starts: Vec<usize>,
    b_word_starts: Vec<usize>,
    steps: Vec<EchoStepDto>,
    segments: Vec<EchoSegmentDto>,
    a_coverage: Vec<u8>,
    b_coverage: Vec<u8>,
    variant_pairs_tried: usize,
}

impl EchoAlignmentDto {
    fn new(echo: &EchoMatch, a: &Line, b: &Line, kind: AlignerKind) -> Self {
        let alignment = &echo.alignment;
        let mut segment_of_step = vec![0; alignment.steps.len()];
        for (k, segment) in alignment.segments.iter().enumerate() {
            segment_of_step[segment.steps.clone()].fill(k);
        }
        let words = |line: &Line| line.words.iter().map(|w| w.word.clone()).collect();
        let phonemes = |p: &[crate::Phoneme]| p.iter().map(ToString::to_string).collect();
        Self {
            aligner: kind.into(),
            raw: alignment.raw,
            self_a: alignment.self_similarity_a,
            self_b: alignment.self_similarity_b,
            similarity: alignment.similarity,
            distance: alignment.distance(),
            a_words: words(a),
            b_words: words(b),
            a_phonemes: phonemes(&echo.a.phonemes),
            b_phonemes: phonemes(&echo.b.phonemes),
            a_word_starts: echo.a.word_starts.clone(),
            b_word_starts: echo.b.word_starts.clone(),
            steps: alignment
                .steps
                .iter()
                .zip(segment_of_step)
                .map(|(step, segment)| EchoStepDto {
                    step: AlignmentStepDto::from(&step.step),
                    score: step.score,
                    a_index: step.a_index,
                    b_index: step.b_index,
                    segment,
                })
                .collect(),
            segments: alignment
                .segments
                .iter()
                .map(|s| EchoSegmentDto {
                    a_start: s.a_range.start,
                    a_end: s.a_range.end,
                    b_start: s.b_range.start,
                    b_end: s.b_range.end,
                    score: s.score,
                    open_cost: s.open_cost,
                    placement: match s.placement {
                        Placement::InPlace => PlacementDto::InPlace,
                        Placement::Moved => PlacementDto::Moved,
                    },
                })
                .collect(),
            a_coverage: alignment.a_coverage.clone(),
            b_coverage: alignment.b_coverage.clone(),
            variant_pairs_tried: echo.variant_pairs_tried,
        }
    }
}

/// A repetition template: a named form or a custom scheme
/// (`A_B__A__B__A__B__AB`, `aab*4`, …).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EchoTemplateDto {
    Duplex,
    Villanelle,
    Pantoum { stanzas: usize },
    Blues { stanzas: usize },
    Custom { scheme: String },
}

impl TryFrom<EchoTemplateDto> for EchoTemplate {
    type Error = crate::ParseEchoTemplateError;

    fn try_from(template: EchoTemplateDto) -> Result<Self, Self::Error> {
        Ok(match template {
            EchoTemplateDto::Duplex => EchoTemplate::duplex(),
            EchoTemplateDto::Villanelle => EchoTemplate::villanelle(),
            EchoTemplateDto::Pantoum { stanzas } => EchoTemplate::pantoum(stanzas),
            EchoTemplateDto::Blues { stanzas } => EchoTemplate::blues(stanzas),
            EchoTemplateDto::Custom { scheme } => EchoTemplate::parse(&scheme)?,
        })
    }
}

#[derive(Serialize, Debug, Tsify)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EchoPairOutcomeDto {
    Scored {
        raw: i64,
        similarity: f32,
        distance: f32,
        passed: bool,
    },
    Error {
        message: String,
    },
}

#[derive(Serialize, Debug, Tsify)]
pub struct EchoPairResultDto {
    leader: usize,
    follower: usize,
    outcome: EchoPairOutcomeDto,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EchoFormResultDto {
    pairs: Vec<EchoPairResultDto>,
    /// Template pairs the poem is too short to contain, as [leader, follower].
    missing_pairs: Vec<[usize; 2]>,
    line_count: usize,
    expected_line_count: Option<usize>,
    line_count_matches: bool,
    passed: bool,
}

#[cfg(test)]
mod tests {
    use super::super::tests::analyzer;
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn identical_lines_score_one_with_every_aligner() {
        let ws = analyzer();
        for kind in [
            AlignerKindDto::SemiGlobal,
            AlignerKindDto::WatermanEggert,
            AlignerKindDto::Jump,
        ] {
            let dto = ws
                .score_echo(
                    "dark demands I call my own",
                    "dark demands I call my own",
                    Some(kind),
                )
                .unwrap();
            assert!(
                (dto.similarity - 1.0).abs() < 1e-6,
                "{kind:?}: {}",
                dto.similarity
            );
            assert_eq!(dto.aligner, kind);
            assert_eq!(dto.a_words.len(), 6);
            assert_eq!(dto.a_word_starts.len(), 6);
            assert!(dto.a_coverage.iter().all(|&c| c == 1));
        }
    }

    #[wasm_bindgen_test]
    fn a_swap_reports_a_moved_segment() {
        let dto = analyzer()
            .score_echo(
                "dark demands I call my own",
                "call my own dark demand sigh",
                Some(AlignerKindDto::Jump),
            )
            .unwrap();
        assert!(dto.similarity > 0.85);
        assert_eq!(
            dto.segments
                .iter()
                .filter(|s| s.placement == PlacementDto::Moved)
                .count(),
            1
        );
        assert!(dto.steps.iter().any(|s| s.segment == 1));
    }

    #[wasm_bindgen_test]
    fn unknown_words_are_errors() {
        assert!(
            analyzer()
                .score_echo("call my own", "call my xyzzy", None)
                .is_err()
        );
    }

    #[wasm_bindgen_test]
    fn a_short_duplex_is_missing_its_closing_pair() {
        let lines: Vec<String> = (0..13)
            .map(|k| format!("the light is gone number {k}"))
            .collect();
        let result = analyzer()
            .check_echo_form(lines, EchoTemplateDto::Duplex, 0.5, None)
            .unwrap();
        assert_eq!(result.missing_pairs, [[0, 13]]);
        assert!(!result.line_count_matches);
        assert_eq!(result.expected_line_count, Some(14));
        assert!(!result.passed);
    }

    #[wasm_bindgen_test]
    fn blank_lines_are_stanza_breaks_and_errors_are_per_pair() {
        let lines = [
            "call my own",
            "call my own",
            "",
            "a nice xyzzy",
            "an ice man",
            "x",
        ]
        .map(String::from)
        .to_vec();
        let result = analyzer()
            .check_echo_form(
                lines,
                EchoTemplateDto::Custom {
                    scheme: "AA bb _".into(),
                },
                0.5,
                None,
            )
            .unwrap();
        assert_eq!(result.line_count, 5);
        assert!(matches!(
            result.pairs[0].outcome,
            EchoPairOutcomeDto::Scored { passed: true, .. }
        ));
        assert!(matches!(
            result.pairs[1].outcome,
            EchoPairOutcomeDto::Error { .. }
        ));
    }

    #[wasm_bindgen_test]
    fn a_bad_custom_template_is_an_error() {
        let result = analyzer().check_echo_form(
            vec![],
            EchoTemplateDto::Custom {
                scheme: "AB-A".into(),
            },
            0.5,
            None,
        );
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn echo_config_round_trips() {
        let mut ws = analyzer();
        assert_eq!(ws.echo_config(), default_echo_config());
        let mut config = ws.echo_config();
        config.gap_open = 250;
        config.aligner = AlignerKindDto::Jump;
        ws.set_echo_config(config);
        assert_eq!(ws.echo_config(), config);
        assert_eq!(EchoConfig::from(config).gaps.gap_open, 250);
        let dto = ws.score_echo("call my own", "call my own", None).unwrap();
        assert_eq!(dto.aligner, AlignerKindDto::Jump);
    }
}
