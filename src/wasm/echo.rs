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
    AlignerKind, EchoConfig, EchoDetector, EchoMatch, EchoNormalization, EchoScheme,
    EchoStrictness, EchoTemplate, GapConfig, Placement, SimilarityConfig, StressWeights,
    StructureConfig,
};
use crate::line::Line;
use crate::{AnalyticConsonantDistance, VowelHexGraph};

/// The default echo config: semi-global's tuned config.
#[wasm_bindgen(js_name = defaultEchoConfig)]
pub fn default_echo_config() -> EchoConfigDto {
    EchoConfig::default().into()
}

/// The config `aligner` was tuned with.
#[wasm_bindgen(js_name = echoConfigFor)]
pub fn echo_config_for(aligner: AlignerKindDto) -> EchoConfigDto {
    EchoConfig::for_aligner(aligner.into()).into()
}

/// The pass mark for `strictness` under `aligner`'s tuned config.
#[wasm_bindgen(js_name = echoThreshold)]
pub fn echo_threshold(strictness: EchoStrictnessDto, aligner: AlignerKindDto) -> f32 {
    EchoStrictness::from(strictness).threshold(aligner.into())
}

#[wasm_bindgen]
impl WasmAnalyzer {
    /// The current echo config.
    #[wasm_bindgen(js_name = echoConfig)]
    pub fn echo_config(&self) -> EchoConfigDto {
        self.echo_config.into()
    }

    /// Replaces the echo config used by `scoreEcho` and `checkEchoForm`. Its
    /// aligner scores every comparison; `echoConfigFor` gives each aligner's
    /// tuned config. Rejects values beyond the config's limits.
    #[wasm_bindgen(js_name = setEchoConfig)]
    pub fn set_echo_config(&mut self, config: EchoConfigDto) -> Result<(), JsError> {
        let config = EchoConfig::from(config);
        config.validate()?;
        self.echo_config = config;
        Ok(())
    }

    /// Scores how strongly line `b` echoes line `a`, with the config's
    /// aligner.
    #[wasm_bindgen(js_name = scoreEcho)]
    pub fn score_echo(&self, a: &str, b: &str) -> Result<EchoAlignmentDto, JsError> {
        let (a_line, b_line) = (Line::new(a, &*self.dict), Line::new(b, &*self.dict));
        let echo = self.echo_detector().compare_lines(&a_line, &b_line)?;
        Ok(EchoAlignmentDto::new(
            &echo,
            &a_line,
            &b_line,
            self.echo_config.aligner,
        ))
    }

    /// Checks a poem's lines against a repetition template. Blank lines
    /// (stanza breaks) are skipped, so indices count non-blank lines.
    ///
    /// Followers pass at `threshold` when given, else at `strictness`'s
    /// measured threshold for the config's aligner (the template's
    /// conventional strictness when omitted). The measured thresholds assume
    /// the aligner's tuned config (`echoConfigFor`).
    #[wasm_bindgen(js_name = checkEchoForm)]
    pub fn check_echo_form(
        &self,
        lines: Vec<String>,
        template: EchoTemplateDto,
        strictness: Option<EchoStrictnessDto>,
        threshold: Option<f32>,
    ) -> Result<EchoFormResultDto, JsError> {
        let template = EchoTemplate::try_from(template)?;
        let expected_line_count = template.expected_line_count();
        let strictness = strictness.map_or_else(|| template.strictness(), EchoStrictness::from);
        let scheme = match threshold {
            Some(threshold) => EchoScheme::with_threshold(template, threshold),
            None => EchoScheme::new(template, strictness),
        };
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
                            raw: score.echo.alignment.raw,
                            similarity: score.echo.alignment.similarity,
                            distance: score.echo.alignment.distance(),
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
            strictness: strictness.into(),
            threshold: result.threshold,
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
}

impl From<AlignerKindDto> for AlignerKind {
    fn from(kind: AlignerKindDto) -> Self {
        match kind {
            AlignerKindDto::SemiGlobal => Self::SemiGlobal,
            AlignerKindDto::WatermanEggert => Self::WatermanEggert,
        }
    }
}

impl From<AlignerKind> for AlignerKindDto {
    fn from(kind: AlignerKind) -> Self {
        match kind {
            AlignerKind::SemiGlobal => Self::SemiGlobal,
            AlignerKind::WatermanEggert => Self::WatermanEggert,
        }
    }
}

/// How closely a poem's repetitions are expected to match. Mirrors
/// [`EchoStrictness`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum EchoStrictnessDto {
    Strict,
    Loose,
}

impl From<EchoStrictnessDto> for EchoStrictness {
    fn from(strictness: EchoStrictnessDto) -> Self {
        match strictness {
            EchoStrictnessDto::Strict => Self::Strict,
            EchoStrictnessDto::Loose => Self::Loose,
        }
    }
}

impl From<EchoStrictness> for EchoStrictnessDto {
    fn from(strictness: EchoStrictness) -> Self {
        match strictness {
            EchoStrictness::Strict => Self::Strict,
            EchoStrictness::Loose => Self::Loose,
        }
    }
}

/// What a raw score is divided by. Mirrors [`EchoNormalization`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum EchoNormalizationDto {
    ShorterSelfSimilarity,
    MeanSelfSimilarity,
}

impl From<EchoNormalizationDto> for EchoNormalization {
    fn from(normalization: EchoNormalizationDto) -> Self {
        match normalization {
            EchoNormalizationDto::ShorterSelfSimilarity => Self::ShorterSelfSimilarity,
            EchoNormalizationDto::MeanSelfSimilarity => Self::MeanSelfSimilarity,
        }
    }
}

impl From<EchoNormalization> for EchoNormalizationDto {
    fn from(normalization: EchoNormalization) -> Self {
        match normalization {
            EchoNormalization::ShorterSelfSimilarity => Self::ShorterSelfSimilarity,
            EchoNormalization::MeanSelfSimilarity => Self::MeanSelfSimilarity,
        }
    }
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
    max_runs: u32,
    normalization: EchoNormalizationDto,
    aligner: AlignerKindDto,
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
            max_runs: to_u32(c.structure.max_runs),
            normalization: c.normalization.into(),
            aligner: c.aligner.into(),
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
                max_runs: c.max_runs as usize,
            },
            normalization: c.normalization.into(),
            aligner: c.aligner.into(),
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
/// words and phonemes (each word under its first pronunciation),
/// where each word starts, the steps, segments and which phonemes are
/// paired.
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
    a_aligned: Vec<bool>,
    b_aligned: Vec<bool>,
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
            a_aligned: alignment.a_aligned.clone(),
            b_aligned: alignment.b_aligned.clone(),
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
            EchoTemplateDto::Pantoum { stanzas } => EchoTemplate::pantoum(stanzas)?,
            EchoTemplateDto::Blues { stanzas } => EchoTemplate::blues(stanzas)?,
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
    /// The strictness judged at (the template's conventional one unless
    /// given) and the threshold actually applied.
    strictness: EchoStrictnessDto,
    threshold: f32,
    passed: bool,
}

#[cfg(test)]
mod tests {
    use super::super::tests::analyzer;
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// An analyzer scoring with `kind` under its tuned config.
    fn analyzer_for(kind: AlignerKindDto) -> WasmAnalyzer {
        let mut ws = analyzer();
        ws.set_echo_config(echo_config_for(kind)).unwrap();
        ws
    }

    const ALIGNERS: [AlignerKindDto; 2] =
        [AlignerKindDto::SemiGlobal, AlignerKindDto::WatermanEggert];

    #[wasm_bindgen_test]
    fn identical_lines_score_one_with_every_aligner() {
        for kind in ALIGNERS {
            let dto = analyzer_for(kind)
                .score_echo("dark demands I call my own", "dark demands I call my own")
                .unwrap();
            assert!(
                (dto.similarity - 1.0).abs() < 1e-6,
                "{kind:?}: {}",
                dto.similarity
            );
            assert_eq!(dto.aligner, kind);
            assert_eq!(dto.a_words.len(), 6);
            assert_eq!(dto.a_word_starts.len(), 6);
            assert!(dto.a_aligned.iter().all(|&x| x));
        }
    }

    #[wasm_bindgen_test]
    fn a_swap_reports_a_moved_segment() {
        let dto = analyzer_for(AlignerKindDto::WatermanEggert)
            .score_echo("dark demands I call my own", "call my own dark demand sigh")
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
                .score_echo("call my own", "call my xyzzy")
                .is_err()
        );
    }

    #[wasm_bindgen_test]
    fn a_short_duplex_is_missing_its_closing_pair() {
        let lines: Vec<String> = (0..13)
            .map(|k| format!("the light is gone number {k}"))
            .collect();
        let result = analyzer()
            .check_echo_form(lines, EchoTemplateDto::Duplex, None, Some(0.5))
            .unwrap();
        assert_eq!(result.missing_pairs, [[0, 13]]);
        assert!(!result.line_count_matches);
        assert_eq!(result.expected_line_count, Some(14));
        assert_eq!(result.threshold, 0.5);
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
                None,
                None,
            )
            .unwrap();
        // A custom template is judged strict by default.
        assert_eq!(result.strictness, EchoStrictnessDto::Strict);
        assert_eq!(
            result.threshold,
            EchoStrictness::Strict.threshold(AlignerKind::SemiGlobal)
        );
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
    fn bad_templates_are_errors_not_panics() {
        let ws = analyzer();
        for template in [
            EchoTemplateDto::Custom {
                scheme: "AB-A".into(),
            },
            EchoTemplateDto::Custom {
                scheme: "aa*200000000".into(),
            },
            EchoTemplateDto::Pantoum { stanzas: 0 },
            EchoTemplateDto::Blues {
                stanzas: usize::MAX,
            },
        ] {
            assert!(
                ws.check_echo_form(vec![], template.clone(), None, Some(0.5))
                    .is_err(),
                "{template:?}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn strictness_picks_the_config_aligners_threshold() {
        let lines: Vec<String> = (0..14).map(|k| format!("the light is gone {k}")).collect();
        // A duplex is conventionally loose.
        let result = analyzer()
            .check_echo_form(lines.clone(), EchoTemplateDto::Duplex, None, None)
            .unwrap();
        assert_eq!(result.strictness, EchoStrictnessDto::Loose);
        assert_eq!(
            result.threshold,
            echo_threshold(EchoStrictnessDto::Loose, AlignerKindDto::SemiGlobal)
        );
        let result = analyzer_for(AlignerKindDto::WatermanEggert)
            .check_echo_form(
                lines,
                EchoTemplateDto::Duplex,
                Some(EchoStrictnessDto::Strict),
                None,
            )
            .unwrap();
        assert_eq!(
            result.threshold,
            echo_threshold(EchoStrictnessDto::Strict, AlignerKindDto::WatermanEggert)
        );
    }

    #[wasm_bindgen_test]
    fn echo_config_round_trips_and_is_validated() {
        let mut ws = analyzer();
        assert_eq!(ws.echo_config(), default_echo_config());
        assert_eq!(
            echo_config_for(AlignerKindDto::SemiGlobal),
            default_echo_config()
        );
        let mut config = echo_config_for(AlignerKindDto::WatermanEggert);
        assert_eq!(
            EchoConfig::from(config),
            EchoConfig::for_aligner(AlignerKind::WatermanEggert)
        );
        config.gap_open = 250;
        ws.set_echo_config(config).unwrap();
        assert_eq!(ws.echo_config(), config);
        assert_eq!(EchoConfig::from(config).gaps.gap_open, 250);
        let dto = ws.score_echo("call my own", "call my own").unwrap();
        assert_eq!(dto.aligner, AlignerKindDto::WatermanEggert);
        // Out of range: rejected, and the previous config kept.
        let mut bad = config;
        bad.vowel_coefficient = u32::MAX;
        assert!(ws.set_echo_config(bad).is_err());
        assert_eq!(ws.echo_config(), config);
    }
}
