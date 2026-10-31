//! WebAssembly bindings for the rhyme & meter analysis library.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    AnalyticConsonantDistanceConfig, Analyzer, CmuDictionary, Compare, ConfiguredDictionary,
    ConsistentScoresOwned, InflectionRule, MeterExceptionConfig, MeterScheme, OwnedAlignmentStep,
    PronouncingDictionary, PronunciationMode, PronunciationOrigin, RhymeComparisonConfig,
    RhymeDistanceConfig, RhymeDistanceScorer, RhymeIntersectionMatchOwned, RhymePairScore,
    RhymeScheme, RhymeScoreNormalization, Stress, TranspositionDirection, VowelHexGraphConfig,
    WordPronunciations, WordTarget,
    line::Line,
    meter::{MeterDeviation, MeterMatchResult, SyllableStress},
    poem::{MeterAnalysis, Poem, PoemLineAnalysis, RhymeAnalysis},
    rhyme_scheme::RhymeCheckResult,
};

/// The concrete, monomorphized scorer the JS layer uses (the library defaults).
type Scorer = RhymeDistanceScorer;

#[wasm_bindgen(js_name = defaultRhymeComparisonConfig)]
pub fn default_rhyme_comparison_config() -> RhymeComparisonConfigDto {
    RhymeComparisonConfig::default().into()
}

// ===========================================================================
// WasmAnalyzer: the long-lived owner of shared, expensive state — the wasm-
// facing analog of the core `Analyzer` (a dictionary + scorer bundle), plus a
// factory for poems that share them.
// ===========================================================================

#[wasm_bindgen(js_name = "Analyzer")]
pub struct WasmAnalyzer {
    dict: Rc<ConfiguredDictionary<CmuDictionary>>,
    scorer: Rc<Scorer>,
}

#[wasm_bindgen]
impl WasmAnalyzer {
    /// Builds an analyzer from raw CMUdict bytes. `pronunciation_mode` defaults
    /// to `"dictionary_only"`; pass `"allow_derived"` to infer missing words.
    /// The policy applies to `pronunciationsFor`, scoring, and every poem opened here.
    #[wasm_bindgen(constructor)]
    pub fn new(
        dict_bytes: &[u8],
        pronunciation_mode: Option<PronunciationModeDto>,
    ) -> Result<WasmAnalyzer, JsError> {
        let dict = CmuDictionary::from_bytes(dict_bytes)?.with_pronunciation_mode(
            pronunciation_mode
                .map(PronunciationMode::from)
                .unwrap_or_default(),
        );
        Ok(WasmAnalyzer {
            dict: Rc::new(dict),
            scorer: Rc::new(Scorer::new()),
        })
    }

    /// Opens a new poem that shares this analyzer's dictionary and scorer.
    /// See [`PoemOptionsDto`] for the schemes and configs it can analyze with;
    /// without options, the poem has no schemes.
    #[wasm_bindgen(js_name = openPoem)]
    pub fn open_poem(
        &self,
        lines: Vec<String>,
        options: Option<PoemOptionsDto>,
    ) -> Result<WasmPoem, JsError> {
        let PoemOptionsDto {
            meter_scheme,
            rhyme_scheme,
            rhyme_comparison_config,
            meter_exception_config,
            meter_mode,
        } = options.unwrap_or_default();
        let meter = match meter_scheme {
            Some(s) => Some(match meter_mode.unwrap_or(MeterModeDto::StressPattern) {
                MeterModeDto::StressPattern => s.parse::<MeterScheme>()?,
                MeterModeDto::SyllableLengths => MeterScheme::from_syllable_counts(&s)?,
            }),
            None => None,
        };
        let rhyme = match rhyme_scheme {
            Some(s) => Some(RhymeScheme::with_config(
                &s,
                rhyme_comparison_config
                    .map(RhymeComparisonConfig::from)
                    .unwrap_or_default(),
            )?),
            None => None,
        };
        let meter_exceptions = match meter_exception_config {
            Some(config) => Some(
                MeterExceptionConfig::new(config.allowed_exceptions, config.lines_per_group)
                    .ok_or_else(|| JsError::new("meter exception line group must be at least 1"))?,
            ),
            None => None,
        };

        let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let poem = Poem::new_with_meter_exceptions(
            &line_refs,
            meter,
            meter_exceptions,
            rhyme,
            &*self.dict,
        );

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

    /// Returns the current vowel-distance config as a JS object.
    #[wasm_bindgen(js_name = vowelConfig)]
    pub fn vowel_config(&self) -> VowelConfigDto {
        VowelConfigDto::from(self.scorer.vowel_config())
    }

    /// Returns the current consonant-distance config as a JS object.
    #[wasm_bindgen(js_name = consonantConfig)]
    pub fn consonant_config(&self) -> ConsonantConfigDto {
        ConsonantConfigDto::from(self.scorer.consonant_config())
    }

    /// Replaces this analyzer's rhyme-distance config.
    ///
    /// This is a "reload", not a live edit: poems opened *after* this call use
    /// the new config; poems already open keep the config they were opened with.
    /// Re-open a poem to apply a config change to it.
    #[wasm_bindgen(js_name = setRhymeConfig)]
    pub fn set_rhyme_config(&mut self, config: RhymeConfigDto) {
        Rc::make_mut(&mut self.scorer).set_config(config.into());
    }

    /// Replaces the vowel-distance config for poems opened afterward.
    #[wasm_bindgen(js_name = setVowelConfig)]
    pub fn set_vowel_config(&mut self, config: VowelConfigDto) {
        Rc::make_mut(&mut self.scorer).set_vowel_config(config.into());
    }

    /// Replaces the consonant-distance config for poems opened afterward.
    #[wasm_bindgen(js_name = setConsonantConfig)]
    pub fn set_consonant_config(&mut self, config: ConsonantConfigDto) {
        Rc::make_mut(&mut self.scorer).set_consonant_config(config.into());
    }

    /// Scans the dictionary for words inside *every* target's distance range
    /// — the multi-target generalization of `findRhymes` (e.g. words between
    /// two hole words). Each match is scored the way `scoreConsistent`
    /// scores it — one shared word pronunciation for all targets — and is a
    /// lean summary: the word, that pronunciation, and one bare score per
    /// target in target order (for per-target pronunciation detail, call
    /// `scoreConsistent` on the words you keep). Matches are sorted by the
    /// sum of their per-target normalized distances, nearest first. All
    /// target words are excluded from the results; an empty target list
    /// returns no matches.
    ///
    /// `normalization` defaults to per-mean-phoneme and `compare` to the
    /// rhyming part. Like `findRhymes`, each call uses the analyzer's
    /// *current* rhyme configs.
    #[wasm_bindgen(js_name = findRhymesIntersection)]
    pub fn find_rhymes_intersection(
        &self,
        targets: Vec<RhymeTargetDto>,
        normalization: Option<RhymeNormalizationDto>,
        compare: Option<CompareDto>,
    ) -> Result<Vec<RhymeIntersectionMatchDto>, JsError> {
        let analyzer = Analyzer::new(&*self.dict, (*self.scorer).clone());
        let word_targets: Vec<WordTarget<'_>> = targets
            .iter()
            .map(|t| WordTarget {
                word: &t.word,
                distance_range: t.min_distance..=t.max_distance,
            })
            .collect();
        let normalization = normalization
            .map(RhymeScoreNormalization::from)
            .unwrap_or_default();
        let matches = analyzer.find_rhymes_intersection_by_words(
            &word_targets,
            normalization,
            compare.map(Compare::from).unwrap_or(Compare::RhymingPart),
        )?;
        Ok(matches
            .iter()
            .map(|m| RhymeIntersectionMatchDto::from_match(m, normalization))
            .collect())
    }

    /// Scores `word` against every word in `targets` using a *single*
    /// pronunciation of `word` — the one minimizing the summed normalized
    /// distance — so the per-target scores are mutually consistent, unlike
    /// repeated `scoreRhyme` calls, which may each pick a different
    /// pronunciation of `word`. Returns `undefined` when no pronunciation of
    /// `word` is comparable to every target. `normalization` defaults to
    /// per-mean-phoneme and `compare` to the rhyming part.
    #[wasm_bindgen(js_name = scoreConsistent)]
    pub fn score_consistent(
        &self,
        word: &str,
        targets: Vec<String>,
        normalization: Option<RhymeNormalizationDto>,
        compare: Option<CompareDto>,
    ) -> Result<Option<ConsistentScoresDto>, JsError> {
        let analyzer = Analyzer::new(&*self.dict, (*self.scorer).clone());
        let target_refs: Vec<&str> = targets.iter().map(String::as_str).collect();
        let normalization = normalization
            .map(RhymeScoreNormalization::from)
            .unwrap_or_default();
        let result = analyzer.calculate_consistent_score_between_words(
            word,
            &target_refs,
            normalization,
            compare.map(Compare::from).unwrap_or(Compare::RhymingPart),
        )?;
        Ok(result
            .as_ref()
            .map(|scores| ConsistentScoresDto::from_scores(scores, normalization)))
    }

    /// Every headword in the dictionary, in unspecified order. One
    /// allocation-heavy call — callers should cache the result.
    #[wasm_bindgen]
    pub fn words(&self) -> Vec<String> {
        self.dict
            .entries()
            .map(|(word, _)| word.to_string())
            .collect()
    }

    /// Every pronunciation of `word`, in dictionary order: cmudict lists a
    /// word's base entry before its (1)/(2) variants — the primary
    /// pronunciation first — and the parser preserves that order, so `[0]`
    /// is the word's most common pronunciation. Each pronunciation is its
    /// ARPAbet phonemes, stress digits included (e.g. `["L", "AY1", "V"]`).
    /// Only exact dictionary entries are returned, regardless of pronunciation mode.
    /// Use [`Self::pronunciations_for`] for optional inference and pronunciation origin.
    #[wasm_bindgen]
    pub fn lookup(&self, word: &str) -> Option<PronunciationsDto> {
        self.dict.lookup(word).map(|pronunciations| {
            PronunciationsDto(
                pronunciations
                    .iter()
                    .map(|phonemes| phonemes.iter().map(ToString::to_string).collect())
                    .collect(),
            )
        })
    }

    /// Returns pronunciations and their origin using this analyzer's pronunciation mode.
    /// In `"allow_derived"` mode, missing words may use productive pronunciation rules.
    #[wasm_bindgen(js_name = pronunciationsFor)]
    pub fn pronunciations_for(&self, word: &str) -> Option<WordPronunciationsDto> {
        self.dict.pronunciations_for(word).map(Into::into)
    }

    /// The distinct syllable counts of `text` — a word or a
    /// whitespace-separated phrase — across its pronunciations, or
    /// `undefined` when any word is unknown.
    #[wasm_bindgen(js_name = syllableCounts)]
    pub fn syllable_counts(&self, text: &str) -> Option<Vec<u32>> {
        Line::new(text, &*self.dict)
            .syllable_counts()
            .map(|counts| counts.into_iter().map(|count| count as u32).collect())
    }

    /// Scores the best pairing of two words, with `a` in the target role and
    /// `b` in the dictionary-word role, so `scoreRhyme(a, b)` agrees with
    /// the score `b` would get in a `findRhymes(a, ..)` scan. Sugar for
    /// `scoreConsistent(b, [a])` reshaped into a pair.
    ///
    /// `normalization` defaults to per-mean-phoneme and `compare` to the
    /// rhyming part. Each call uses the analyzer's *current* rhyme configs.
    #[wasm_bindgen(js_name = scoreRhyme)]
    pub fn score_rhyme(
        &self,
        a: &str,
        b: &str,
        normalization: Option<RhymeNormalizationDto>,
        compare: Option<CompareDto>,
    ) -> Result<Option<RhymePairDto>, JsError> {
        let analyzer = Analyzer::new(&*self.dict, (*self.scorer).clone());
        let normalization = normalization
            .map(RhymeScoreNormalization::from)
            .unwrap_or_default();
        let consistent = analyzer.calculate_consistent_score_between_words(
            b,
            &[a],
            normalization,
            compare.map(Compare::from).unwrap_or(Compare::RhymingPart),
        )?;
        Ok(consistent.map(|c| RhymePairDto {
            a_pronunciation: c.best_scored_targets[0]
                .0
                .iter()
                .map(ToString::to_string)
                .collect(),
            b_pronunciation: c
                .best_subject_pronunciation
                .iter()
                .map(ToString::to_string)
                .collect(),
            score: RhymePairScoreDto::from_score(&c.best_scored_targets[0].1, normalization),
        }))
    }
}

// ===========================================================================
// WasmPoem: a cheap per-poem handle. Holds Rc clones of the shared state, so
// opening the Nth poem costs a refcount bump, not another 3.8 MB dictionary.
// ===========================================================================

#[wasm_bindgen(js_name = "Poem")]
pub struct WasmPoem {
    dict: Rc<ConfiguredDictionary<CmuDictionary>>,
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum PronunciationModeDto {
    DictionaryOnly,
    AllowDerived,
}

impl From<PronunciationModeDto> for PronunciationMode {
    fn from(mode: PronunciationModeDto) -> Self {
        match mode {
            PronunciationModeDto::DictionaryOnly => Self::DictionaryOnly,
            PronunciationModeDto::AllowDerived => Self::AllowDerived,
        }
    }
}

/// A word's pronunciations in dictionary order — each one its ARPAbet
/// phonemes. A newtype, so it crosses to JS as a plain `string[][]`.
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct PronunciationsDto(pub Vec<Vec<String>>);

/// Pronunciations plus their dictionary or derivation provenance.
#[derive(Serialize, Debug, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi)]
pub struct WordPronunciationsDto {
    pronunciations: Vec<Vec<String>>,
    origin: PronunciationOriginDto,
}

impl From<WordPronunciations<'_>> for WordPronunciationsDto {
    fn from(word_pronunciations: WordPronunciations<'_>) -> Self {
        WordPronunciationsDto {
            pronunciations: word_pronunciations
                .pronunciations
                .iter()
                .map(|phonemes| phonemes.iter().map(ToString::to_string).collect())
                .collect(),
            origin: word_pronunciations.origin.into(),
        }
    }
}

/// The source of a pronunciation. A derived result includes the
/// dictionary base and its ordered, outward-applied rule chain.
#[derive(Serialize, Debug, PartialEq, Eq, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PronunciationOriginDto {
    Dictionary {
        normalized: bool,
    },
    Derived {
        dictionary_base: String,
        rules: Vec<InflectionRuleDto>,
    },
}

impl From<PronunciationOrigin> for PronunciationOriginDto {
    fn from(origin: PronunciationOrigin) -> Self {
        match origin {
            PronunciationOrigin::Dictionary { normalized } => {
                PronunciationOriginDto::Dictionary { normalized }
            }
            PronunciationOrigin::Derived {
                dictionary_base,
                rules,
            } => PronunciationOriginDto::Derived {
                dictionary_base,
                rules: rules.into_iter().map(Into::into).collect(),
            },
        }
    }
}

#[derive(Serialize, Debug, PartialEq, Eq, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum InflectionRuleDto {
    ApostropheS,
    SApostrophe,
    S,
    Ves,
    Ly,
    Ing,
    Ed,
    Er,
    Est,
}

impl From<InflectionRule> for InflectionRuleDto {
    fn from(rule: InflectionRule) -> Self {
        match rule {
            InflectionRule::ApostropheS => InflectionRuleDto::ApostropheS,
            InflectionRule::SApostrophe => InflectionRuleDto::SApostrophe,
            InflectionRule::S => InflectionRuleDto::S,
            InflectionRule::Ves => InflectionRuleDto::Ves,
            InflectionRule::Ly => InflectionRuleDto::Ly,
            InflectionRule::Ing => InflectionRuleDto::Ing,
            InflectionRule::Ed => InflectionRuleDto::Ed,
            InflectionRule::Er => InflectionRuleDto::Er,
            InflectionRule::Est => InflectionRuleDto::Est,
        }
    }
}

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

/// Flattens the three core layers — `NotApplicable` / `Err` / `Ok(Match|…|NoMatch)`
/// — into one tagged union: `{ status: "match" }`,
/// `{ status: "match_with_exceptions", exceptions }`,
/// `{ status: "no_match", failures, exceptions }`, `{ status: "error", message }`, or
/// `{ status: "not_applicable" }`. `failures` and `exceptions` are deviations of the
/// line's least-deviating reading; `exceptions` are the ones its allowance covered.
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MeterDto {
    NotApplicable,
    Error {
        message: String,
    },
    Match,
    MatchWithExceptions {
        exceptions: Vec<MeterDeviationDto>,
    },
    NoMatch {
        failures: Vec<MeterDeviationDto>,
        exceptions: Vec<MeterDeviationDto>,
    },
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
                Ok(MeterMatchResult::MatchWithExceptions { exceptions }) => {
                    MeterDto::MatchWithExceptions {
                        exceptions: exceptions.iter().map(MeterDeviationDto::from).collect(),
                    }
                }
                Ok(MeterMatchResult::NoMatch {
                    failures,
                    exceptions,
                }) => MeterDto::NoMatch {
                    failures: failures.iter().map(MeterDeviationDto::from).collect(),
                    exceptions: exceptions.iter().map(MeterDeviationDto::from).collect(),
                },
            },
        }
    }
}

/// Options for `openPoem`. Every field may be omitted, and an omitted scheme
/// leaves that dimension unanalyzed.
#[derive(Deserialize, Debug, Default, Tsify)]
#[tsify(from_wasm_abi)]
pub struct PoemOptionsDto {
    /// A meter scheme: stress patterns such as `"x/x/x/x/x/"`, or with
    /// `meter_mode: "syllable_lengths"`, line lengths such as `"8\n7-9\n5"`.
    #[tsify(optional)]
    meter_scheme: Option<String>,
    /// A rhyme scheme in the core parser's format, e.g. `"ABAB"`.
    #[tsify(optional)]
    rhyme_scheme: Option<String>,
    /// How rhyme candidates are compared; defaults to the core config.
    #[tsify(optional)]
    rhyme_comparison_config: Option<RhymeComparisonConfigDto>,
    /// Allows syllable-level meter deviations in fixed groups of lines.
    #[tsify(optional)]
    meter_exception_config: Option<MeterExceptionConfigDto>,
    /// How `meter_scheme` is read; defaults to `"stress_pattern"`.
    #[tsify(optional)]
    meter_mode: Option<MeterModeDto>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum MeterModeDto {
    StressPattern,
    SyllableLengths,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MeterExceptionConfigDto {
    allowed_exceptions: usize,
    lines_per_group: usize,
}

#[derive(Serialize, Debug, Tsify)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeterDeviationDto {
    StressMismatch {
        word_index: usize,
        syllable_index: usize,
        expected: StressDto,
        actual: PronunciationStressDto,
    },
    ExtraSyllable {
        word_index: usize,
        syllable_index: usize,
        actual: PronunciationStressDto,
    },
    /// `before_word_index` is the word the gap comes before, or the line's
    /// word count when the gap follows its last word.
    MissingSyllable {
        before_word_index: usize,
        meter_syllable_index: usize,
        expected: StressDto,
    },
}

impl From<&MeterDeviation> for MeterDeviationDto {
    fn from(deviation: &MeterDeviation) -> Self {
        match deviation {
            MeterDeviation::StressMismatch {
                word_index,
                syllable_index,
                expected,
                actual,
            } => Self::StressMismatch {
                word_index: *word_index,
                syllable_index: *syllable_index,
                expected: (*expected).into(),
                actual: (*actual).into(),
            },
            MeterDeviation::ExtraSyllable {
                word_index,
                syllable_index,
                actual,
            } => Self::ExtraSyllable {
                word_index: *word_index,
                syllable_index: *syllable_index,
                actual: (*actual).into(),
            },
            MeterDeviation::MissingSyllable {
                before_word_index,
                meter_syllable_index,
                expected,
            } => Self::MissingSyllable {
                before_word_index: *before_word_index,
                meter_syllable_index: *meter_syllable_index,
                expected: (*expected).into(),
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

#[derive(Serialize, Debug, Tsify)]
#[serde(rename_all = "snake_case")]
pub enum PronunciationStressDto {
    Primary,
    Secondary,
    Unstressed,
}

impl From<Stress> for PronunciationStressDto {
    fn from(stress: Stress) -> Self {
        match stress {
            Stress::Primary => Self::Primary,
            Stress::Secondary => Self::Secondary,
            Stress::Unstressed => Self::Unstressed,
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
                        raw_distance: distance.score.raw_distance,
                        syllable_count: distance.syllable_count,
                        normalization: distance.normalization.into(),
                        normalization_denominator: distance.normalization.denominator(
                            distance.score.target_syllable_count,
                            distance.score.leader_len,
                            distance.score.target_len,
                        ),
                        normalized_distance: distance.score.normalized(distance.normalization),
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
    normalization: RhymeNormalizationDto,
    normalization_denominator: f32,
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

// ---- find_rhymes -----------------------------------------------------------

/// Which portion of the target word a dictionary scan compares against.
/// Mirrors [`Compare`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum CompareDto {
    WholeWord,
    RhymingPart,
}

impl From<CompareDto> for Compare {
    fn from(compare: CompareDto) -> Self {
        match compare {
            CompareDto::WholeWord => Self::WholeWord,
            CompareDto::RhymingPart => Self::RhymingPart,
        }
    }
}

#[derive(Serialize, Debug, Tsify)]
pub struct RhymePairScoreDto {
    raw_distance: u32,
    normalization_denominator: f32,
    normalized_distance: f32,
}

impl RhymePairScoreDto {
    fn from_score(score: &RhymePairScore, normalization: RhymeScoreNormalization) -> Self {
        Self {
            raw_distance: score.raw_distance,
            normalization_denominator: normalization.denominator(
                score.target_syllable_count,
                score.leader_len,
                score.target_len,
            ),
            normalized_distance: score.normalized(normalization),
        }
    }
}

/// One target of a `findRhymesIntersection` scan. Mirrors [`RhymeIntersectionTarget`].
#[derive(Deserialize, Debug, Clone, Tsify)]
#[tsify(from_wasm_abi)]
pub struct RhymeTargetDto {
    word: String,
    min_distance: f32,
    max_distance: f32,
}

/// A dictionary word inside every target's distance range — a lean scan
/// summary. For per-target pronunciation detail, ask `scoreConsistent`
/// about the words you keep. Mirrors [`RhymeIntersectionMatchOwned`].
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct RhymeIntersectionMatchDto {
    word: String,
    /// ARPAbet phonemes of the word's pronunciation all scores share.
    pronunciation: Vec<String>,
    /// `scores[i]` pairs `pronunciation` with `targets[i]`.
    scores: Vec<RhymePairScoreDto>,
}

impl RhymeIntersectionMatchDto {
    fn from_match(m: &RhymeIntersectionMatchOwned, normalization: RhymeScoreNormalization) -> Self {
        RhymeIntersectionMatchDto {
            word: m.word.to_string(),
            pronunciation: m
                .scores
                .best_subject_pronunciation
                .iter()
                .map(ToString::to_string)
                .collect(),
            scores: m
                .scores
                .best_scored_targets
                .iter()
                .map(|(_, score)| RhymePairScoreDto::from_score(score, normalization))
                .collect(),
        }
    }
}

/// The target pronunciation a score paired against, and the score itself.
/// Mirrors one tuple in `ConsistentScores::best_scored_targets`.
#[derive(Serialize, Debug, Tsify)]
pub struct ScoredPronunciationDto {
    /// ARPAbet phonemes of the target's winning comparison slice.
    pronunciation: Vec<String>,
    score: RhymePairScoreDto,
}

/// One pronunciation of a word scored against several targets — every score
/// shares `pronunciation`. Mirrors [`ConsistentScores`].
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ConsistentScoresDto {
    /// ARPAbet phonemes of the subject-word pronunciation all scores share.
    pronunciation: Vec<String>,
    /// `scores[i]` pairs `pronunciation` with `targets[i]`; each entry's
    /// pronunciation is the *target's* winning one.
    scores: Vec<ScoredPronunciationDto>,
}

impl ConsistentScoresDto {
    fn from_scores(c: &ConsistentScoresOwned, normalization: RhymeScoreNormalization) -> Self {
        ConsistentScoresDto {
            pronunciation: c
                .best_subject_pronunciation
                .iter()
                .map(ToString::to_string)
                .collect(),
            scores: c
                .best_scored_targets
                .iter()
                .map(|(pronunciation, score)| ScoredPronunciationDto {
                    pronunciation: pronunciation.iter().map(ToString::to_string).collect(),
                    score: RhymePairScoreDto::from_score(score, normalization),
                })
                .collect(),
        }
    }
}

/// The best pairing of two words — `scoreConsistent` with one target,
/// reshaped for the single-pair caller.
#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct RhymePairDto {
    /// ARPAbet phonemes of the winning `a` comparison slice.
    a_pronunciation: Vec<String>,
    /// The full `b` pronunciation containing the comparison slice that won.
    b_pronunciation: Vec<String>,
    score: RhymePairScoreDto,
}

// ---- Config (round-trips both directions: get + set) ----------------------

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum RhymeNormalizationDto {
    None,
    PerSyllable,
    PerMeanPhoneme,
}

impl From<RhymeScoreNormalization> for RhymeNormalizationDto {
    fn from(normalization: RhymeScoreNormalization) -> Self {
        match normalization {
            RhymeScoreNormalization::None => Self::None,
            RhymeScoreNormalization::PerSyllable => Self::PerSyllable,
            RhymeScoreNormalization::PerMeanPhoneme => Self::PerMeanPhoneme,
        }
    }
}

impl From<RhymeNormalizationDto> for RhymeScoreNormalization {
    fn from(normalization: RhymeNormalizationDto) -> Self {
        match normalization {
            RhymeNormalizationDto::None => Self::None,
            RhymeNormalizationDto::PerSyllable => Self::PerSyllable,
            RhymeNormalizationDto::PerMeanPhoneme => Self::PerMeanPhoneme,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RhymeComparisonConfigDto {
    threshold: f32,
    normalization: RhymeNormalizationDto,
}

impl From<RhymeComparisonConfig> for RhymeComparisonConfigDto {
    fn from(config: RhymeComparisonConfig) -> Self {
        Self {
            threshold: config.threshold,
            normalization: config.normalization.into(),
        }
    }
}

impl From<RhymeComparisonConfigDto> for RhymeComparisonConfig {
    fn from(config: RhymeComparisonConfigDto) -> Self {
        Self {
            threshold: config.threshold,
            normalization: config.normalization.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Tsify)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct VowelConfigDto {
    vowel_stress_penalty: u32,
    vowel_distance_coefficient: u32,
    er_ah_distance: u32,
}

impl From<VowelHexGraphConfig> for VowelConfigDto {
    fn from(c: VowelHexGraphConfig) -> Self {
        Self {
            vowel_stress_penalty: c.vowel_stress_penalty,
            vowel_distance_coefficient: c.vowel_distance_coefficient,
            er_ah_distance: c.er_ah_distance,
        }
    }
}

impl From<VowelConfigDto> for VowelHexGraphConfig {
    fn from(c: VowelConfigDto) -> Self {
        Self {
            vowel_stress_penalty: c.vowel_stress_penalty,
            vowel_distance_coefficient: c.vowel_distance_coefficient,
            er_ah_distance: c.er_ah_distance,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ConsonantConfigDto {
    place_distance_coefficient: u32,
    unrelated_penalty: u32,
    voiced_penalty: u32,
    sibilance_penalty: u32,
    r_l_distance: u32,
    w_v_distance: u32,
    affricate_sibilant_fricative_penalty: u32,
    affricate_plosive_penalty: u32,
    affricate_non_sibilant_fricative_penalty: u32,
}

impl From<AnalyticConsonantDistanceConfig> for ConsonantConfigDto {
    fn from(c: AnalyticConsonantDistanceConfig) -> Self {
        Self {
            place_distance_coefficient: c.place_distance_coefficient,
            unrelated_penalty: c.unrelated_penalty,
            voiced_penalty: c.voiced_penalty,
            sibilance_penalty: c.sibilance_penalty,
            r_l_distance: c.r_l_distance,
            w_v_distance: c.w_v_distance,
            affricate_sibilant_fricative_penalty: c.affricate_sibilant_fricative_penalty,
            affricate_plosive_penalty: c.affricate_plosive_penalty,
            affricate_non_sibilant_fricative_penalty: c.affricate_non_sibilant_fricative_penalty,
        }
    }
}

impl From<ConsonantConfigDto> for AnalyticConsonantDistanceConfig {
    fn from(c: ConsonantConfigDto) -> Self {
        Self {
            place_distance_coefficient: c.place_distance_coefficient,
            unrelated_penalty: c.unrelated_penalty,
            voiced_penalty: c.voiced_penalty,
            sibilance_penalty: c.sibilance_penalty,
            r_l_distance: c.r_l_distance,
            w_v_distance: c.w_v_distance,
            affricate_sibilant_fricative_penalty: c.affricate_sibilant_fricative_penalty,
            affricate_plosive_penalty: c.affricate_plosive_penalty,
            affricate_non_sibilant_fricative_penalty: c.affricate_non_sibilant_fricative_penalty,
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
        WasmAnalyzer::new(dict_bytes(), None).expect("embedded dictionary should parse")
    }

    // ---- construction -----------------------------------------------------

    #[wasm_bindgen_test]
    fn analyzer_from_valid_dict_bytes_succeeds() {
        assert!(WasmAnalyzer::new(dict_bytes(), None).is_ok());
    }

    #[wasm_bindgen_test]
    fn analyzer_from_malformed_dict_bytes_errors() {
        let bad = b"NOT_A_VALID_LINE_WITHOUT_A_SEPARATOR\n";
        assert!(
            WasmAnalyzer::new(bad, None).is_err(),
            "malformed dictionary bytes should surface as an Err, not panic"
        );
    }

    #[wasm_bindgen_test]
    fn pronunciation_mode_applies_to_queries_and_poem_edits() {
        for mode in [
            None,
            Some(PronunciationModeDto::DictionaryOnly),
            Some(PronunciationModeDto::AllowDerived),
        ] {
            let allow_derived = mode == Some(PronunciationModeDto::AllowDerived);
            let ws = WasmAnalyzer::new(b"KETTLE  K EH1 T AH0 L\n", mode).unwrap();
            assert!(ws.lookup("kettle").is_some());
            assert!(ws.lookup("kettle's").is_none());
            assert_eq!(ws.pronunciations_for("kettle's").is_some(), allow_derived);
            assert_eq!(ws.syllable_counts("kettle's").is_some(), allow_derived);
            assert_eq!(
                ws.score_rhyme("kettle's", "kettles", None, None).is_ok(),
                allow_derived
            );
            assert_eq!(
                ws.find_rhymes_intersection(
                    vec![RhymeTargetDto {
                        word: "kettles".into(),
                        min_distance: 0.0,
                        max_distance: 1000.0
                    }],
                    None,
                    None,
                )
                .is_ok(),
                allow_derived
            );
            assert_eq!(ws.words(), vec!["kettle"]);

            let mut poem = ws
                .open_poem(
                    vec!["kettle's".into()],
                    Some(PoemOptionsDto {
                        meter_scheme: Some("/x".into()),
                        ..Default::default()
                    }),
                )
                .unwrap();
            assert_eq!(
                matches!(poem.meter_analysis(0).unwrap(), MeterDto::Match),
                allow_derived
            );
            poem.update_line(0, "kettles").unwrap();
            assert_eq!(
                matches!(poem.meter_analysis(0).unwrap(), MeterDto::Match),
                allow_derived
            );
            poem.insert_line(1, "kettle's").unwrap();
            assert_eq!(
                matches!(poem.meter_analysis(1).unwrap(), MeterDto::Match),
                allow_derived
            );
        }
    }

    #[wasm_bindgen_test]
    fn pronunciations_and_syllable_counts_include_missing_possessives() {
        let ws = WasmAnalyzer::new(dict_bytes(), Some(PronunciationModeDto::AllowDerived)).unwrap();

        assert_eq!(
            ws.pronunciations_for("kettle's").unwrap().pronunciations[0],
            ["K", "EH1", "T", "AH0", "L", "Z"]
        );
        assert_eq!(
            ws.pronunciations_for("kettle’s").unwrap().pronunciations,
            ws.pronunciations_for("kettle's").unwrap().pronunciations
        );
        assert_eq!(ws.syllable_counts("kettle's"), Some(vec![2]));
    }

    #[wasm_bindgen_test]
    fn pronunciations_report_dictionary_and_derived_origins() {
        let ws = WasmAnalyzer::new(dict_bytes(), Some(PronunciationModeDto::AllowDerived)).unwrap();

        assert_eq!(
            ws.pronunciations_for("hello").unwrap().origin,
            PronunciationOriginDto::Dictionary { normalized: false }
        );
        assert_eq!(
            ws.pronunciations_for("cat’s").unwrap().origin,
            PronunciationOriginDto::Dictionary { normalized: true }
        );
        assert_eq!(
            ws.pronunciations_for("kettle’s").unwrap().origin,
            PronunciationOriginDto::Derived {
                dictionary_base: "kettle".to_string(),
                rules: vec![InflectionRuleDto::ApostropheS],
            }
        );

        let sparse = WasmAnalyzer::new(
            b"PLAIN  P L EY1 N\n",
            Some(PronunciationModeDto::AllowDerived),
        )
        .unwrap();
        let recursively_derived = sparse.pronunciations_for("plainer's").unwrap();
        assert_eq!(
            recursively_derived.origin,
            PronunciationOriginDto::Derived {
                dictionary_base: "plain".to_string(),
                rules: vec![InflectionRuleDto::Er, InflectionRuleDto::ApostropheS,],
            }
        );
        assert_eq!(
            recursively_derived.pronunciations[0],
            ["P", "L", "EY1", "N", "ER0", "Z"]
        );

        let ves = WasmAnalyzer::new(
            b"KNIFE  N AY1 F\n",
            Some(PronunciationModeDto::AllowDerived),
        )
        .unwrap();
        assert_eq!(
            ves.pronunciations_for("knives").unwrap().origin,
            PronunciationOriginDto::Derived {
                dictionary_base: "knife".to_string(),
                rules: vec![InflectionRuleDto::Ves],
            }
        );

        let plural_possessive = WasmAnalyzer::new(
            b"KETTLE  K EH1 T AH0 L\n",
            Some(PronunciationModeDto::AllowDerived),
        )
        .unwrap();
        assert_eq!(
            plural_possessive
                .pronunciations_for("kettles'")
                .unwrap()
                .origin,
            PronunciationOriginDto::Derived {
                dictionary_base: "kettle".to_string(),
                rules: vec![InflectionRuleDto::S, InflectionRuleDto::SApostrophe],
            }
        );
    }

    #[wasm_bindgen_test]
    fn pronunciations_meter_and_rhyme_support_er_and_est_inflections() {
        let ws = WasmAnalyzer::new(dict_bytes(), Some(PronunciationModeDto::AllowDerived)).unwrap();

        assert_eq!(
            ws.pronunciations_for("sorer").unwrap().pronunciations[0],
            ["S", "AO1", "R", "ER0"]
        );
        assert!(
            ws.pronunciations_for("plainest")
                .unwrap()
                .pronunciations
                .iter()
                .any(|pronunciation| { pronunciation == &["P", "L", "EY1", "N", "IH0", "S", "T"] })
        );
        assert_eq!(ws.syllable_counts("plainest sorer"), Some(vec![4]));

        let mut poem = ws
            .open_poem(
                vec!["forest".into(), "sorest".into()],
                Some(PoemOptionsDto {
                    meter_scheme: Some("/x".into()),
                    rhyme_scheme: Some("AA".into()),
                    ..Default::default()
                }),
            )
            .unwrap();
        assert!(matches!(poem.meter_analysis(1).unwrap(), MeterDto::Match));
        assert!(matches!(
            poem.rhyme_analysis(1).unwrap(),
            RhymeDto::Follower { passed: true, .. }
        ));
    }

    // ---- scheme-parse errors propagate as Err, not panics -----------------

    #[wasm_bindgen_test]
    fn open_poem_rejects_invalid_meter_scheme() {
        let ws = analyzer();
        // 'q' isn't a valid meter character (only x, /, _, ( and ) are).
        let result = ws.open_poem(
            vec!["cat".into()],
            Some(PoemOptionsDto {
                meter_scheme: Some("q".into()),
                ..Default::default()
            }),
        );
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn open_poem_rejects_invalid_rhyme_scheme() {
        let ws = analyzer();
        // '1' isn't a valid rhyme character (only letters and '_' are).
        let result = ws.open_poem(
            vec!["cat".into()],
            Some(PoemOptionsDto {
                rhyme_scheme: Some("1".into()),
                ..Default::default()
            }),
        );
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn open_poem_accepts_a_syllable_length_scheme() {
        let mut poem = analyzer()
            .open_poem(
                vec!["cat dog".into(), "cat dog bird".into(), "cat".into()],
                Some(PoemOptionsDto {
                    meter_scheme: Some("2\n2-3\n1".into()),
                    meter_mode: Some(MeterModeDto::SyllableLengths),
                    ..Default::default()
                }),
            )
            .unwrap();

        for index in 0..3 {
            assert!(matches!(
                poem.meter_analysis(index).unwrap(),
                MeterDto::Match
            ));
        }
    }

    #[wasm_bindgen_test]
    fn open_poem_rejects_an_invalid_syllable_length_scheme() {
        let result = analyzer().open_poem(
            vec!["cat".into()],
            Some(PoemOptionsDto {
                meter_scheme: Some("5\n9-7".into()),
                meter_mode: Some(MeterModeDto::SyllableLengths),
                ..Default::default()
            }),
        );

        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn open_poem_rejects_a_zero_line_meter_exception_group() {
        let ws = analyzer();
        let result = ws.open_poem(
            vec!["hello".into()],
            Some(PoemOptionsDto {
                meter_scheme: Some("//".into()),
                meter_exception_config: Some(MeterExceptionConfigDto {
                    allowed_exceptions: 1,
                    lines_per_group: 0,
                }),
                ..Default::default()
            }),
        );
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn meter_exception_dto_distinguishes_allowed_and_over_budget_deviations() {
        let mut poem = analyzer()
            .open_poem(
                vec![
                    "hello".into(),
                    "hello".into(),
                    "hello".into(),
                    "hello".into(),
                ],
                Some(PoemOptionsDto {
                    meter_scheme: Some("//".into()),
                    meter_exception_config: Some(MeterExceptionConfigDto {
                        allowed_exceptions: 2,
                        lines_per_group: 3,
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();

        assert!(matches!(
            poem.meter_analysis(0).unwrap(),
            MeterDto::MatchWithExceptions { exceptions } if exceptions.len() == 1
        ));
        assert!(matches!(
            poem.meter_analysis(1).unwrap(),
            MeterDto::MatchWithExceptions { exceptions } if exceptions.len() == 1
        ));
        assert!(matches!(
            poem.meter_analysis(2).unwrap(),
            MeterDto::NoMatch { exceptions, .. } if exceptions.is_empty()
        ));
        assert!(matches!(
            poem.meter_analysis(3).unwrap(),
            MeterDto::MatchWithExceptions { exceptions } if exceptions.len() == 1
        ));
    }

    #[wasm_bindgen_test]
    fn meter_no_match_dto_reports_each_failed_syllable() {
        let mut poem = analyzer()
            .open_poem(
                vec!["hello".into()],
                Some(PoemOptionsDto {
                    meter_scheme: Some("x".into()),
                    ..Default::default()
                }),
            )
            .unwrap();

        let MeterDto::NoMatch {
            failures,
            exceptions,
        } = poem.meter_analysis(0).unwrap()
        else {
            panic!("expected a meter mismatch");
        };
        // Only HELLO's first syllable fits the one-syllable meter.
        assert!(exceptions.is_empty());
        assert!(matches!(
            failures.as_slice(),
            [MeterDeviationDto::ExtraSyllable {
                word_index: 0,
                syllable_index: 1,
                actual: PronunciationStressDto::Primary,
            }]
        ));
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
                Some(PoemOptionsDto {
                    rhyme_scheme: Some("AA".into()),
                    ..Default::default()
                }),
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
                Some(PoemOptionsDto {
                    rhyme_scheme: Some("AA".into()),
                    ..Default::default()
                }),
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
    // Config setters update the scorer copy used by poems opened *after* the
    // change. Poems opened before keep their old scorer snapshot.

    #[wasm_bindgen_test]
    fn component_configs_round_trip_without_resetting_each_other() {
        let mut ws = analyzer();
        let rhyme_config = ws.rhyme_config();

        let mut vowel_config = ws.vowel_config();
        vowel_config.vowel_distance_coefficient = 9;
        vowel_config.er_ah_distance = 4;
        ws.set_vowel_config(vowel_config);

        let mut consonant_config = ws.consonant_config();
        consonant_config.place_distance_coefficient = 3;
        consonant_config.voiced_penalty = 7;
        consonant_config.sibilance_penalty = 11;
        ws.set_consonant_config(consonant_config);

        assert_eq!(ws.rhyme_config(), rhyme_config);
        assert_eq!(ws.vowel_config(), vowel_config);
        assert_eq!(ws.consonant_config(), consonant_config);
    }

    fn distance_with_normalization(normalization: RhymeNormalizationDto) -> RhymeDistanceDto {
        let mut poem = analyzer()
            .open_poem(
                vec!["cat".into(), "cot".into()],
                Some(PoemOptionsDto {
                    rhyme_scheme: Some("AA".into()),
                    rhyme_comparison_config: Some(RhymeComparisonConfigDto {
                        threshold: 1000.0,
                        normalization,
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();

        let RhymeDto::Follower { distance, .. } = poem.rhyme_analysis(1).unwrap() else {
            panic!("expected a follower rhyme result");
        };
        distance
    }

    #[wasm_bindgen_test]
    fn comparison_config_controls_normalization() {
        assert_eq!(
            default_rhyme_comparison_config().normalization,
            RhymeNormalizationDto::PerMeanPhoneme
        );

        let none = distance_with_normalization(RhymeNormalizationDto::None);
        let per_syllable = distance_with_normalization(RhymeNormalizationDto::PerSyllable);
        let per_mean_phoneme = distance_with_normalization(RhymeNormalizationDto::PerMeanPhoneme);

        assert!(none.raw_distance > 0);
        assert_eq!(none.raw_distance, per_syllable.raw_distance);
        assert_eq!(none.raw_distance, per_mean_phoneme.raw_distance);
        assert_eq!(none.normalization_denominator, 1.0);
        assert_eq!(per_syllable.normalization_denominator, 1.0);
        assert_eq!(per_mean_phoneme.normalization_denominator, 2.0);
        assert_eq!(none.normalized_distance, none.raw_distance as f32);
        assert_eq!(
            per_mean_phoneme.normalized_distance,
            none.raw_distance as f32 / 2.0
        );
    }

    #[wasm_bindgen_test]
    fn rhyme_config_reload_affects_poems_opened_after() {
        let mut ws = analyzer();

        // CAT (K AE1 T) / COT (K AA1 T): rhyming parts differ only by vowel
        // (AE vs AA, adjacent in the vowel hex graph), so raising
        // vowel_coefficient should raise the raw distance.
        let mut poem_before = ws
            .open_poem(
                vec!["cat".into(), "cot".into()],
                Some(PoemOptionsDto {
                    rhyme_scheme: Some("AA".into()),
                    ..Default::default()
                }),
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
                Some(PoemOptionsDto {
                    rhyme_scheme: Some("AA".into()),
                    ..Default::default()
                }),
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

    // ---- find_rhymes_intersection -------------------------------------------

    #[wasm_bindgen_test]
    fn find_rhymes_intersection_returns_per_target_scores() {
        let ws = analyzer();
        let matches = ws
            .find_rhymes_intersection(
                vec![
                    RhymeTargetDto {
                        word: "cat".into(),
                        min_distance: 0.0,
                        max_distance: 0.0,
                    },
                    RhymeTargetDto {
                        word: "hat".into(),
                        min_distance: 0.0,
                        max_distance: 0.0,
                    },
                ],
                None,
                None,
            )
            .unwrap();

        let bat = matches
            .iter()
            .find(|m| m.word == "bat")
            .expect("BAT rhymes perfectly with CAT and HAT");
        // One shared pronunciation of the match, one bare score per target.
        assert_eq!(bat.pronunciation, ["B", "AE1", "T"]);
        assert_eq!(bat.scores.len(), 2);
        assert!(bat.scores.iter().all(|s| s.raw_distance == 0));
    }

    #[wasm_bindgen_test]
    fn find_rhymes_intersection_unknown_target_errors() {
        let ws = analyzer();
        let result = ws.find_rhymes_intersection(
            vec![RhymeTargetDto {
                word: "xyzzy".into(),
                min_distance: 0.0,
                max_distance: 1000.0,
            }],
            None,
            None,
        );
        assert!(result.is_err());
    }

    // ---- score_rhyme --------------------------------------------------------

    #[wasm_bindgen_test]
    fn score_rhyme_scores_a_pair_with_owned_pronunciations() {
        let ws = analyzer();
        let pair = ws
            .score_rhyme("cot", "caught", None, Some(CompareDto::WholeWord))
            .unwrap()
            .expect("one-syllable words are always comparable");

        assert_eq!(pair.score.raw_distance, 0);
        assert_eq!(pair.score.normalized_distance, 0.0);
        assert_eq!(pair.a_pronunciation, ["K", "AA1", "T"]);
        assert_eq!(pair.b_pronunciation, ["K", "AA1", "T"]);
    }

    #[wasm_bindgen_test]
    fn score_rhyme_unknown_word_surfaces_as_error() {
        let ws = analyzer();
        assert!(ws.score_rhyme("cat", "xyzzy", None, None).is_err());
    }
}
