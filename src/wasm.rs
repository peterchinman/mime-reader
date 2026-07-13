//! WebAssembly bindings for the rhyme & meter analysis library.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    CmuDictionary, MeterScheme, OwnedAlignmentStep, RhymeDistanceConfig, RhymeDistanceScorer,
    RhymeScheme, TranspositionDirection,
    meter::{MeterMatchResult, MeterMismatch, SyllableStress},
    poem::{MeterAnalysis, Poem, PoemLineAnalysis, RhymeAnalysis},
    rhyme_scheme::RhymeCheckResult,
};

/// The concrete, monomorphized scorer the JS layer uses (the library defaults).
type Scorer = RhymeDistanceScorer;

// ===========================================================================
// WasmAnalyzer: the long-lived owner of shared, expensive state — the wasm-
// facing analog of the core `Analyzer` (a dictionary + scorer bundle), plus a
// factory for poems that share them.
// ===========================================================================

#[wasm_bindgen(js_name = "Analyzer")]
pub struct WasmAnalyzer {
    dict: Rc<CmuDictionary>,
    scorer: Rc<Scorer>,
}

#[wasm_bindgen]
impl WasmAnalyzer {
    /// Builds an analyzer from the raw CMUdict bytes
    #[wasm_bindgen(constructor)]
    pub fn new(dict_bytes: &[u8]) -> Result<WasmAnalyzer, JsError> {
        let dict = CmuDictionary::from_bytes(dict_bytes)?;
        Ok(WasmAnalyzer {
            dict: Rc::new(dict),
            scorer: Rc::new(Scorer::new()),
        })
    }

    /// Opens a new poem that shares this analyzer's dictionary and scorer.
    ///
    /// `meter_scheme` / `rhyme_scheme` are the same string formats the core
    /// parsers accept (e.g. meter `"x/x/x/x/x/"`, rhyme `"ABAB"`). Pass
    /// `undefined` to leave a dimension unanalyzed.
    #[wasm_bindgen(js_name = openPoem)]
    pub fn open_poem(
        &self,
        lines: Vec<String>,
        meter_scheme: Option<String>,
        rhyme_scheme: Option<String>,
        rhyme_threshold: Option<f32>,
    ) -> Result<WasmPoem, JsError> {
        let meter = match meter_scheme {
            Some(s) => Some(s.parse::<MeterScheme>()?),
            None => None,
        };
        let rhyme = match rhyme_scheme {
            Some(s) => Some(RhymeScheme::new(&s, rhyme_threshold.unwrap_or(0.5))?),
            None => None,
        };

        let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let poem = Poem::new(&line_refs, meter, rhyme, &*self.dict);

        Ok(WasmPoem {
            dict: self.dict.clone(),
            scorer: self.scorer.clone(),
            poem,
        })
    }

    /// Returns the current rhyme-distance config as a JS object (e.g. to
    /// populate config controls on a tuning page).
    #[wasm_bindgen(js_name = rhymeConfig)]
    pub fn rhyme_config(&self) -> RhymeConfigDto {
        RhymeConfigDto::from(self.scorer.config())
    }

    /// Rebuilds this analyzer's rhyme-distance scorer with the given config.
    ///
    /// This is a "reload", not a live edit: poems opened *after* this call use
    /// the new config; poems already open keep the config they were opened with.
    /// Re-open a poem to apply a config change to it.
    #[wasm_bindgen(js_name = setRhymeConfig)]
    pub fn set_rhyme_config(&mut self, config: RhymeConfigDto) {
        let mut scorer = Scorer::new();
        scorer.set_config(config.into());
        self.scorer = Rc::new(scorer);
    }
}

// ===========================================================================
// WasmPoem: a cheap per-poem handle. Holds Rc clones of the shared state, so
// opening the Nth poem costs a refcount bump, not another 3.8 MB dictionary.
// ===========================================================================

#[wasm_bindgen(js_name = "Poem")]
pub struct WasmPoem {
    dict: Rc<CmuDictionary>,
    // The config this poem was opened with. A later `WasmAnalyzer::set_rhyme_config`
    // swaps the analyzer's scorer but not this Rc, so the poem keeps its config
    // until re-opened.
    scorer: Rc<Scorer>,
    poem: Poem,
}

#[wasm_bindgen]
impl WasmPoem {
    #[wasm_bindgen(js_name = updateLine)]
    pub fn update_line(&mut self, index: usize, text: &str) -> Result<(), JsError> {
        self.poem.update_line(index, text, &*self.dict)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = insertLine)]
    pub fn insert_line(&mut self, index: usize, text: &str) -> Result<(), JsError> {
        self.poem.insert_line(index, text, &*self.dict)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = deleteLine)]
    pub fn delete_line(&mut self, index: usize) -> Result<(), JsError> {
        self.poem.delete_line(index)?;
        Ok(())
    }

    /// Meter + rhyme analysis for one line, as a `{ meter, rhyme }` JS object.
    #[wasm_bindgen(js_name = lineAnalysis)]
    pub fn line_analysis(&mut self, index: usize) -> Result<LineAnalysisDto, JsError> {
        let analysis = self.poem.line_analysis(index, &*self.scorer)?;
        Ok(LineAnalysisDto::from(analysis))
    }

    #[wasm_bindgen(js_name = meterAnalysis)]
    pub fn meter_analysis(&mut self, index: usize) -> Result<MeterDto, JsError> {
        let analysis = self.poem.meter_analysis(index)?;
        Ok(MeterDto::from(analysis))
    }

    #[wasm_bindgen(js_name = rhymeAnalysis)]
    pub fn rhyme_analysis(&mut self, index: usize) -> Result<RhymeDto, JsError> {
        let analysis = self.poem.rhyme_analysis(index, &*self.scorer)?;
        Ok(RhymeDto::from(analysis))
    }

    /// Detailed step-by-step rhyme alignment for a scored follower.
    ///
    /// This is intentionally separate from `rhymeAnalysis`: most callers only
    /// need pass/fail and distance numbers, while this is diagnostic detail.
    /// Returns `undefined` unless the line is a scored rhyme follower.
    #[wasm_bindgen(js_name = rhymeAlignment)]
    pub fn rhyme_alignment(
        &mut self,
        index: usize,
    ) -> Result<Option<Vec<RhymeAlignmentStepDto>>, JsError> {
        let analysis = self.poem.rhyme_analysis(index, &*self.scorer)?;
        let alignment = match analysis {
            RhymeAnalysis::Ready(Ok(RhymeCheckResult::Follower { distance, .. })) => {
                let costs = self.scorer.step_costs(&distance.alignment);
                Some(
                    distance
                        .alignment
                        .iter()
                        .zip(costs)
                        .map(|(step, cost)| RhymeAlignmentStepDto {
                            step: AlignmentStepDto::from(step),
                            cost,
                        })
                        .collect(),
                )
            }
            _ => None,
        };
        Ok(alignment)
    }
}

// ===========================================================================
// DTOs — owned, serializable mirrors of the borrowed core result types.
// ===========================================================================

/// Combined per-line analysis. Mirrors [`PoemLineAnalysis`].
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct LineAnalysisDto {
    meter: MeterDto,
    rhyme: RhymeDto,
}

impl From<PoemLineAnalysis<'_>> for LineAnalysisDto {
    fn from(a: PoemLineAnalysis<'_>) -> Self {
        LineAnalysisDto {
            meter: a.meter.into(),
            rhyme: a.rhyme.into(),
        }
    }
}

// ---- Meter ----------------------------------------------------------------

/// Flattens the three core layers — `NotApplicable` / `Err` / `Ok(Match|NoMatch)`
/// — into one tagged union: `{ status: "match" }`, `{ status: "no_match", reasons }`,
/// `{ status: "error", message }`, or `{ status: "not_applicable" }`.
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MeterDto {
    NotApplicable,
    Error { message: String },
    Match,
    NoMatch { reasons: Vec<MeterMismatchDto> },
}

impl From<MeterAnalysis<'_>> for MeterDto {
    fn from(a: MeterAnalysis<'_>) -> Self {
        match a {
            MeterAnalysis::NotApplicable => MeterDto::NotApplicable,
            MeterAnalysis::Ready(result) => match result {
                Err(e) => MeterDto::Error {
                    message: e.to_string(),
                },
                Ok(MeterMatchResult::Match) => MeterDto::Match,
                Ok(MeterMatchResult::NoMatch { reasons }) => MeterDto::NoMatch {
                    reasons: reasons.iter().map(MeterMismatchDto::from).collect(),
                },
            },
        }
    }
}

#[derive(Serialize, Debug, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeterMismatchDto {
    TooShort {
        word_index: usize,
    },
    TooLong {
        remaining_syllables: usize,
    },
    StressMismatch {
        word_index: usize,
        expected: Vec<StressDto>,
    },
}

impl From<&MeterMismatch> for MeterMismatchDto {
    fn from(m: &MeterMismatch) -> Self {
        match m {
            MeterMismatch::MeterTooShort {
                word_index_failed_at,
            } => MeterMismatchDto::TooShort {
                word_index: *word_index_failed_at,
            },
            MeterMismatch::MeterTooLong {
                remaining_syllables_in_meter,
            } => MeterMismatchDto::TooLong {
                remaining_syllables: *remaining_syllables_in_meter,
            },
            MeterMismatch::WordsStressMismatch {
                expected_stress_pattern_of_word,
                word_index_failed_at,
            } => MeterMismatchDto::StressMismatch {
                word_index: *word_index_failed_at,
                expected: expected_stress_pattern_of_word
                    .iter()
                    .map(|s| StressDto::from(*s))
                    .collect(),
            },
        }
    }
}

#[derive(Serialize, Debug, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum StressDto {
    Stressed,
    Unstressed,
    Either,
}

impl From<SyllableStress> for StressDto {
    fn from(s: SyllableStress) -> Self {
        match s {
            SyllableStress::Stressed => StressDto::Stressed,
            SyllableStress::Unstressed => StressDto::Unstressed,
            SyllableStress::Either => StressDto::Either,
        }
    }
}

// ---- Rhyme ----------------------------------------------------------------

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RhymeDto {
    NotApplicable,
    Error {
        message: String,
    },
    Leader,
    Unconstrained,
    Follower {
        leader_index: usize,
        passed: bool,
        distance: RhymeDistanceDto,
    },
}

impl From<RhymeAnalysis<'_>> for RhymeDto {
    fn from(a: RhymeAnalysis<'_>) -> Self {
        match a {
            RhymeAnalysis::NotApplicable => RhymeDto::NotApplicable,
            RhymeAnalysis::Ready(result) => match result {
                Err(e) => RhymeDto::Error {
                    message: e.to_string(),
                },
                Ok(RhymeCheckResult::Leader) => RhymeDto::Leader,
                Ok(RhymeCheckResult::Unconstrained) => RhymeDto::Unconstrained,
                Ok(RhymeCheckResult::Follower {
                    leader_index,
                    distance,
                    passed,
                }) => RhymeDto::Follower {
                    leader_index: *leader_index,
                    passed: *passed,
                    distance: RhymeDistanceDto {
                        raw_distance: distance.raw_distance,
                        syllable_count: distance.syllable_count,
                        normalized_distance: distance.normalized_distance,
                    },
                },
            },
        }
    }
}

#[derive(Serialize, Debug, Tsify)]
pub struct RhymeDistanceDto {
    raw_distance: u32,
    syllable_count: usize,
    normalized_distance: f32,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct RhymeAlignmentStepDto {
    step: AlignmentStepDto,
    cost: u32,
}

#[derive(Serialize, Debug, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlignmentStepDto {
    Match {
        a: String,
        b: String,
    },
    Substitution {
        a: String,
        b: String,
    },
    /// Insertion/deletion: exactly one side is present.
    Indel {
        a: Option<String>,
        b: Option<String>,
    },
    Transposition {
        a: String,
        b: String,
        direction: TranspositionDirectionDto,
    },
}

#[derive(Serialize, Debug, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum TranspositionDirectionDto {
    /// This phoneme matches a later/right column in the other sequence.
    Right,
    /// This phoneme matches an earlier/left column in the other sequence.
    Left,
}

impl From<&OwnedAlignmentStep> for AlignmentStepDto {
    fn from(step: &OwnedAlignmentStep) -> Self {
        match step {
            OwnedAlignmentStep::Match { a, b } => AlignmentStepDto::Match {
                a: a.to_string(),
                b: b.to_string(),
            },
            OwnedAlignmentStep::Substitution { a, b } => AlignmentStepDto::Substitution {
                a: a.to_string(),
                b: b.to_string(),
            },
            OwnedAlignmentStep::Indel { a, b } => AlignmentStepDto::Indel {
                a: a.as_ref().map(|p| p.to_string()),
                b: b.as_ref().map(|p| p.to_string()),
            },
            OwnedAlignmentStep::Transposition { a, b, direction } => {
                AlignmentStepDto::Transposition {
                    a: a.to_string(),
                    b: b.to_string(),
                    direction: match direction {
                        TranspositionDirection::Right => TranspositionDirectionDto::Right,
                        TranspositionDirection::Left => TranspositionDirectionDto::Left,
                    },
                }
            }
        }
    }
}

// ---- Config (round-trips both directions: get + set) ----------------------

#[derive(Serialize, Deserialize, Debug, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RhymeConfigDto {
    vowel_to_consonant_mismatch: u32,
    vowel_coefficient: u32,
    consonant_coefficient: u32,
    vowel_indel_penalty: u32,
    consonant_indel_penalty: u32,
    consonant_repeated_penalty: u32,
    transposition_cost: u32,
}

impl From<RhymeDistanceConfig> for RhymeConfigDto {
    fn from(c: RhymeDistanceConfig) -> Self {
        RhymeConfigDto {
            vowel_to_consonant_mismatch: c.vowel_to_consonant_mismatch,
            vowel_coefficient: c.vowel_coefficient,
            consonant_coefficient: c.consonant_coefficient,
            vowel_indel_penalty: c.vowel_indel_penalty,
            consonant_indel_penalty: c.consonant_indel_penalty,
            consonant_repeated_penalty: c.consonant_repeated_penalty,
            transposition_cost: c.transposition_cost,
        }
    }
}

impl From<RhymeConfigDto> for RhymeDistanceConfig {
    fn from(c: RhymeConfigDto) -> Self {
        RhymeDistanceConfig {
            vowel_to_consonant_mismatch: c.vowel_to_consonant_mismatch,
            vowel_coefficient: c.vowel_coefficient,
            consonant_coefficient: c.consonant_coefficient,
            vowel_indel_penalty: c.vowel_indel_penalty,
            consonant_indel_penalty: c.consonant_indel_penalty,
            consonant_repeated_penalty: c.consonant_repeated_penalty,
            transposition_cost: c.transposition_cost,
        }
    }
}

// ===========================================================================
// Tests. Run with `wasm-pack test --node` (not `cargo test` — JsValue/JsError
// only mean anything inside a JS host, so these run compiled to wasm32 under
// Node). No web_sys/DOM usage here, so `--node` is sufficient; no browser
// needed.
//
// These deliberately don't re-test meter/rhyme *correctness* — that's the
// native suite's job (see poem.rs, rhyme_scheme.rs, meter.rs). What's tested
// here is adapter-specific: does a parse/lookup error surface as a JS-catchable
// error instead of a panic, does the emitted JSON have the tagged-union shape
// a JS consumer would actually rely on, and does a config reload affect poems
// opened afterward.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    // Embedded at compile time: wasm32-unknown-unknown has no filesystem, so
    // `CmuDictionary::load` (std::fs) isn't usable in a wasm test; `from_bytes`
    // plus `include_bytes!` (a compile-time, host-side read) is the equivalent
    // of the native tests' `Dictionary::load(DICT_PATH)`.
    fn dict_bytes() -> &'static [u8] {
        include_bytes!("../data/CMUdict/cmudict-0.7b")
    }

    fn analyzer() -> WasmAnalyzer {
        WasmAnalyzer::new(dict_bytes()).expect("embedded dictionary should parse")
    }

    // ---- construction -----------------------------------------------------

    #[wasm_bindgen_test]
    fn analyzer_from_valid_dict_bytes_succeeds() {
        assert!(WasmAnalyzer::new(dict_bytes()).is_ok());
    }

    #[wasm_bindgen_test]
    fn analyzer_from_malformed_dict_bytes_errors() {
        let bad = b"NOT_A_VALID_LINE_WITHOUT_A_SEPARATOR\n";
        assert!(
            WasmAnalyzer::new(bad).is_err(),
            "malformed dictionary bytes should surface as an Err, not panic"
        );
    }

    // ---- scheme-parse errors propagate as Err, not panics -----------------

    #[wasm_bindgen_test]
    fn open_poem_rejects_invalid_meter_scheme() {
        let ws = analyzer();
        // 'q' isn't a valid meter character (only x, /, _, ( and ) are).
        let result = ws.open_poem(vec!["cat".into()], Some("q".into()), None, None);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn open_poem_rejects_invalid_rhyme_scheme() {
        let ws = analyzer();
        // '1' isn't a valid rhyme character (only letters and '_' are).
        let result = ws.open_poem(vec!["cat".into()], None, Some("1".into()), None);
        assert!(result.is_err());
    }

    // ---- returned DTO shape ------------------------------------------------
    //
    // The methods now return the concrete DTO enums, so we match on them
    // directly. The exact JS/TS wire shape (the `status` tags, the snake_case
    // keys) is generated from these same serde attributes by tsify — verified
    // by inspecting the emitted `.d.ts`, not at runtime here.

    #[wasm_bindgen_test]
    fn line_analysis_shape_for_perfect_rhyme() {
        let ws = analyzer();
        // CAT (K AE1 T) / HAT (HH AE1 T) share an identical rhyming part, so
        // this is a zero-distance, passing rhyme regardless of config.
        let mut poem = ws
            .open_poem(
                vec!["cat".into(), "hat".into()],
                None,
                Some("AA".into()),
                None,
            )
            .unwrap();

        let meter = poem.meter_analysis(0).unwrap();
        assert!(matches!(meter, MeterDto::NotApplicable), "got {meter:?}");

        let leader = poem.rhyme_analysis(0).unwrap();
        assert!(matches!(leader, RhymeDto::Leader), "got {leader:?}");

        match poem.rhyme_analysis(1).unwrap() {
            RhymeDto::Follower {
                leader_index,
                passed,
                distance,
            } => {
                assert_eq!(leader_index, 0);
                assert!(passed);
                assert_eq!(distance.raw_distance, 0);
                assert_eq!(distance.normalized_distance, 0.0);
            }
            other => panic!("expected a follower rhyme result, got {other:?}"),
        }

        let alignment = poem
            .rhyme_alignment(1)
            .unwrap()
            .expect("follower has alignment");
        match &alignment[..] {
            [
                RhymeAlignmentStepDto {
                    step: AlignmentStepDto::Match { a: a0, b: b0 },
                    cost: cost0,
                },
                RhymeAlignmentStepDto {
                    step: AlignmentStepDto::Match { a: a1, b: b1 },
                    cost: cost1,
                },
            ] => {
                assert_eq!((a0.as_str(), b0.as_str(), *cost0), ("AE1", "AE1", 0));
                assert_eq!((a1.as_str(), b1.as_str(), *cost1), ("T", "T", 0));
            }
            other => panic!("expected two match steps, got {other:?}"),
        }
    }

    #[wasm_bindgen_test]
    fn rhyme_alignment_is_explicit_and_costs_sum_to_raw_distance() {
        let ws = analyzer();
        // cat/cot: rhyming parts AE1 T vs AA1 T — a vowel substitution then a
        // consonant match, so a non-trivial cost breakdown.
        let mut poem = ws
            .open_poem(
                vec!["cat".into(), "cot".into()],
                None,
                Some("AA".into()),
                None,
            )
            .unwrap();

        // Non-followers (here, the leader at line 0) have no alignment detail.
        assert!(poem.rhyme_alignment(0).unwrap().is_none());

        let RhymeDto::Follower { distance, .. } = poem.rhyme_analysis(1).unwrap() else {
            panic!("expected a follower rhyme result");
        };
        let alignment = poem
            .rhyme_alignment(1)
            .unwrap()
            .expect("follower has alignment");

        // One cost per alignment step, summing to the reported raw distance.
        assert_eq!(
            alignment.iter().map(|step| step.cost).sum::<u32>(),
            distance.raw_distance
        );
        // The vowel substitution carries cost; the trailing consonant match is free.
        assert!(alignment[0].cost > 0, "vowel substitution should cost > 0");
        assert_eq!(
            alignment.last().unwrap().cost,
            0,
            "trailing consonant match is free"
        );
    }

    // ---- config reload semantics -------------------------------------------
    //
    // `set_rhyme_config` rebuilds the scorer; a poem opened *after* the change
    // must reflect the new config. (Poems opened before keep their old config —
    // that's the "reload, not live edit" contract.)

    #[wasm_bindgen_test]
    fn rhyme_config_reload_affects_poems_opened_after() {
        let mut ws = analyzer();

        // CAT (K AE1 T) / COT (K AA1 T): rhyming parts differ only by vowel
        // (AE vs AA, adjacent in the vowel hex graph), so raising
        // vowel_coefficient should raise the raw distance.
        let mut poem_before = ws
            .open_poem(
                vec!["cat".into(), "cot".into()],
                None,
                Some("AA".into()),
                None,
            )
            .unwrap();
        let RhymeDto::Follower {
            distance: distance_before,
            ..
        } = poem_before.rhyme_analysis(1).unwrap()
        else {
            panic!("expected a follower rhyme result");
        };

        let mut config = ws.rhyme_config();
        config.vowel_coefficient *= 100;
        ws.set_rhyme_config(config);

        let mut poem_after = ws
            .open_poem(
                vec!["cat".into(), "cot".into()],
                None,
                Some("AA".into()),
                None,
            )
            .unwrap();
        let RhymeDto::Follower {
            distance: distance_after,
            ..
        } = poem_after.rhyme_analysis(1).unwrap()
        else {
            panic!("expected a follower rhyme result");
        };

        assert!(
            distance_after.raw_distance > distance_before.raw_distance,
            "raising vowel_coefficient should raise raw distance for a genuine vowel \
             mismatch (cat/cot): before={distance_before:?}, after={distance_after:?}"
        );
    }
}
