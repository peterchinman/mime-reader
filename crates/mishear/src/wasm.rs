//! Bindings for the web app (`web/`): an engine built from fetched bytes,
//! every knob as a field of one request, and the catalog of presets and
//! defaults so the page never restates them.

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::cost::{CostConfig, CostModel};
use crate::deform::VowelChart;
use crate::engine::{Engine, MishearConfig};
use crate::lexicon::{BIGRAM_PRIOR, CommonWords, Frequency, parse_frequencies};
use crate::phone::Ph;
use crate::search::{self, SearchConfig};
use crate::spec::{self, StepSpec};
use crate::{accents, knobs};

/// What the lexicon is built from; changing any of these rebuilds it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct LexiconOptions {
    /// The most frequent words kept.
    pub vocabulary: usize,
    /// Every CMUdict pronunciation, or only the first.
    pub all_pronunciations: bool,
    /// Keep only words the system word list has in lowercase (when it was
    /// supplied).
    pub common_words: bool,
    /// Bigram smoothing μ.
    pub bigram_prior: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StepDto {
    Accent(String),
    Knob(String),
    Vowels(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct DeformationDto {
    pub steps: Vec<StepDto>,
    pub intensity: f32,
    pub seed: u32,
}

/// [`CostConfig`], flattened.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CostDto {
    pub vowel_substitution: f32,
    pub consonant_substitution: f32,
    pub stressed_vowel_indel: f32,
    pub unstressed_vowel_indel: f32,
    pub consonant_indel: f32,
    pub stress_mismatch: f32,
    /// `null`: no swaps.
    pub swap: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SearchDto {
    pub results: usize,
    /// Every match within this cost; `null` for the exact top `results`.
    pub within: Option<f32>,
    pub lm_weight: f32,
    pub word_penalty: f32,
    pub bigrams: bool,
    pub shortlist: usize,
    pub distinct_sounds: bool,
    pub novelty: f32,
    pub function_share: f32,
    /// How far the top-N search may raise its bound.
    pub max_bound: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Deform the text, then hear it back.
    Mishear,
    /// Hear ARPAbet phonemes (no deformation, no novelty).
    Phonemes,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RequestDto {
    pub mode: Mode,
    pub text: String,
    pub deformation: DeformationDto,
    pub search: SearchDto,
    pub cost: CostDto,
}

#[derive(Serialize, Debug, Tsify)]
pub struct WordDto {
    pub text: String,
    pub phonemes: Vec<String>,
    pub deformed: Vec<String>,
}

#[derive(Serialize, Debug, Tsify)]
pub struct HeardWordDto {
    pub text: String,
    /// The input phonemes it covers: `[start, end)`.
    pub start: usize,
    pub end: usize,
}

#[derive(Serialize, Debug, Tsify)]
pub struct HearingDto {
    pub text: String,
    pub cost: f32,
    pub phonetic: f32,
    pub shared: usize,
    pub words: Vec<HeardWordDto>,
}

#[derive(Serialize, Debug, Tsify)]
pub struct StatsDto {
    pub bound: f32,
    pub rounds: usize,
    pub nodes: f64,
    pub edges: usize,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ResultDto {
    /// The line's words (empty in phoneme mode).
    pub words: Vec<WordDto>,
    /// What was heard: the deformed phonemes, flattened.
    pub input: Vec<String>,
    pub hearings: Vec<HearingDto>,
    pub stats: StatsDto,
    pub deformation: String,
}

#[derive(Serialize, Debug, Tsify)]
pub struct PreviewVowelDto {
    pub symbol: String,
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub moved_start: [f32; 2],
    pub moved_end: [f32; 2],
    pub becomes: String,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct PreviewDto {
    pub vowels: Vec<PreviewVowelDto>,
}

#[derive(Serialize, Debug, Tsify)]
pub struct PresetDto {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CatalogDto {
    pub accents: Vec<PresetDto>,
    pub knobs: Vec<String>,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DefaultsDto {
    pub lexicon: LexiconOptions,
    pub search: SearchDto,
    pub cost: CostDto,
}

#[derive(Serialize, Debug, Tsify)]
#[tsify(into_wasm_abi)]
pub struct LexiconInfoDto {
    pub words: usize,
    pub trie_nodes: usize,
    pub bigrams: bool,
    pub common_words_available: bool,
}

impl From<CostDto> for CostConfig {
    fn from(c: CostDto) -> Self {
        Self {
            vowel_substitution: c.vowel_substitution,
            consonant_substitution: c.consonant_substitution,
            stressed_vowel_indel: c.stressed_vowel_indel,
            unstressed_vowel_indel: c.unstressed_vowel_indel,
            consonant_indel: c.consonant_indel,
            stress_mismatch: c.stress_mismatch,
            swap: c.swap,
        }
    }
}

impl From<CostConfig> for CostDto {
    fn from(c: CostConfig) -> Self {
        Self {
            vowel_substitution: c.vowel_substitution,
            consonant_substitution: c.consonant_substitution,
            stressed_vowel_indel: c.stressed_vowel_indel,
            unstressed_vowel_indel: c.unstressed_vowel_indel,
            consonant_indel: c.consonant_indel,
            stress_mismatch: c.stress_mismatch,
            swap: c.swap,
        }
    }
}

fn steps(d: &DeformationDto) -> Vec<StepSpec> {
    d.steps
        .iter()
        .map(|s| match s {
            StepDto::Accent(n) => StepSpec::Accent(n.clone()),
            StepDto::Knob(n) => StepSpec::Knob(n.clone()),
            StepDto::Vowels(n) => StepSpec::Vowels(n.clone()),
        })
        .collect()
}

fn spelled(phonemes: &[Ph]) -> Vec<String> {
    phonemes.iter().map(ToString::to_string).collect()
}

/// The engine and what it was built from.
#[wasm_bindgen]
pub struct MishearApp {
    engine: Engine,
    frequencies: Vec<Frequency>,
    bigrams: Option<String>,
    common: Option<CommonWords>,
    lexicon: LexiconOptions,
    cost: Option<CostDto>,
}

#[wasm_bindgen]
impl MishearApp {
    /// Builds the engine from CMUdict's bytes, the word-frequency list, and
    /// optionally the bigram counts and the system word list.
    #[wasm_bindgen(constructor)]
    pub fn new(
        cmudict: &[u8],
        frequencies: &str,
        bigrams: Option<String>,
        common_words: Option<String>,
        lexicon: LexiconOptions,
    ) -> Result<MishearApp, JsError> {
        let dict = mime_reader::CmuDictionary::from_bytes(cmudict)?;
        let frequencies = parse_frequencies(frequencies).map_err(|e| JsError::new(&e))?;
        let common = common_words.map(|text| CommonWords::parse(&text));
        let filter = common.as_ref().filter(|_| lexicon.common_words);
        let engine = Engine::new(
            dict,
            &frequencies,
            lexicon.vocabulary,
            lexicon.all_pronunciations,
            filter,
        );
        let mut app = Self {
            engine,
            frequencies,
            bigrams,
            common,
            lexicon,
            cost: None,
        };
        app.load_bigrams()?;
        Ok(app)
    }

    fn load_bigrams(&mut self) -> Result<(), JsError> {
        if let Some(text) = &self.bigrams {
            self.engine
                .lexicon
                .parse_bigrams(text, self.lexicon.bigram_prior)
                .map_err(|e| JsError::new(&e))?;
        }
        Ok(())
    }

    /// Rebuilds the lexicon if `lexicon` differs from the current options.
    #[wasm_bindgen(js_name = setLexicon)]
    pub fn set_lexicon(&mut self, lexicon: LexiconOptions) -> Result<(), JsError> {
        if lexicon == self.lexicon {
            return Ok(());
        }
        let rebuild = lexicon.vocabulary != self.lexicon.vocabulary
            || lexicon.all_pronunciations != self.lexicon.all_pronunciations
            || lexicon.common_words != self.lexicon.common_words;
        self.lexicon = lexicon;
        if rebuild {
            let filter = self.common.as_ref().filter(|_| lexicon.common_words);
            self.engine.rebuild_lexicon(
                &self.frequencies,
                lexicon.vocabulary,
                lexicon.all_pronunciations,
                filter,
            );
        }
        self.load_bigrams()
    }

    #[wasm_bindgen(js_name = lexiconInfo)]
    pub fn lexicon_info(&self) -> LexiconInfoDto {
        LexiconInfoDto {
            words: self.engine.lexicon.len(),
            trie_nodes: self.engine.lexicon.node_count(),
            bigrams: self.engine.lexicon.has_bigrams(),
            common_words_available: self.common.is_some(),
        }
    }

    /// Runs one request: deform and hear text, or hear phonemes.
    pub fn run(&mut self, request: RequestDto) -> Result<ResultDto, JsError> {
        if self.cost != Some(request.cost) {
            self.engine.cost = CostModel::new(request.cost.into());
            self.cost = Some(request.cost);
        }
        let s = request.search;
        let search_config = SearchConfig {
            results: s.results.max(1),
            bound: s.within.unwrap_or(2.0),
            lm_weight: s.lm_weight,
            word_penalty: s.word_penalty,
            bigrams: s.bigrams,
            shortlist: s.shortlist,
            distinct_sounds: s.distinct_sounds,
            ..SearchConfig::default()
        };
        let max_bound = s.within.unwrap_or(s.max_bound);
        let lexicon = &self.engine.lexicon;
        let hearing = |words: &[crate::lexicon::WordId], spans: &[std::ops::Range<usize>]| {
            words
                .iter()
                .zip(spans)
                .map(|(&w, span)| HeardWordDto {
                    text: lexicon.word(w).to_string(),
                    start: span.start,
                    end: span.end,
                })
                .collect::<Vec<_>>()
        };
        match request.mode {
            Mode::Phonemes => {
                let input = Ph::parse_all(&request.text)
                    .filter(|p| !p.is_empty())
                    .ok_or_else(|| JsError::new("expected ARPAbet phonemes, e.g. K IH1 S"))?;
                let (matches, stats) = search::best_matches(
                    &input,
                    lexicon,
                    &self.engine.cost,
                    &search_config,
                    1.5,
                    max_bound,
                );
                Ok(ResultDto {
                    words: Vec::new(),
                    input: spelled(&input),
                    hearings: matches
                        .iter()
                        .map(|m| HearingDto {
                            text: m.text(lexicon),
                            cost: m.cost,
                            phonetic: m.phonetic,
                            shared: 0,
                            words: hearing(&m.words, &m.spans),
                        })
                        .collect(),
                    stats: StatsDto {
                        bound: stats.bound,
                        rounds: stats.rounds,
                        nodes: stats.nodes as f64,
                        edges: stats.edges,
                    },
                    deformation: String::new(),
                })
            }
            Mode::Mishear => {
                let deformation = spec::deformation(
                    &steps(&request.deformation),
                    request.deformation.intensity,
                    u64::from(request.deformation.seed),
                )
                .map_err(|e| JsError::new(&e))?;
                let config = MishearConfig {
                    search: search_config,
                    novelty: s.novelty,
                    function_word_share: s.function_share,
                    max_bound,
                };
                let m = self
                    .engine
                    .mishear(&request.text, &deformation, &config)
                    .map_err(|e| JsError::new(&e))?;
                Ok(ResultDto {
                    words: m
                        .reading
                        .words
                        .iter()
                        .zip(&m.reading.phonemes)
                        .zip(&m.deformed)
                        .map(|((w, before), after)| WordDto {
                            text: w.clone(),
                            phonemes: spelled(before),
                            deformed: spelled(after),
                        })
                        .collect(),
                    input: spelled(&m.deformed.concat()),
                    hearings: m
                        .heard
                        .iter()
                        .map(|h| HearingDto {
                            text: h.text.clone(),
                            cost: h.cost,
                            phonetic: h.phonetic,
                            shared: h.shared,
                            words: h
                                .words
                                .iter()
                                .zip(&h.spans)
                                .map(|(w, span)| HeardWordDto {
                                    text: w.clone(),
                                    start: span.start,
                                    end: span.end,
                                })
                                .collect(),
                        })
                        .collect(),
                    stats: StatsDto {
                        bound: m.stats.bound,
                        rounds: m.stats.rounds,
                        nodes: m.stats.nodes as f64,
                        edges: m.stats.edges,
                    },
                    deformation: deformation.name,
                })
            }
        }
    }

    /// Where the deformation's vowel moves take every vowel on the chart.
    pub fn preview(&self, deformation: DeformationDto) -> Result<PreviewDto, JsError> {
        let d = spec::deformation(&steps(&deformation), 1.0, 0).map_err(|e| JsError::new(&e))?;
        let pt = |p: crate::deform::Point| [p.0, p.1];
        Ok(PreviewDto {
            vowels: VowelChart::general_american()
                .preview(&d)
                .into_iter()
                .map(|v| PreviewVowelDto {
                    symbol: v.symbol.to_string(),
                    start: pt(v.start),
                    end: pt(v.end),
                    moved_start: pt(v.moved_start),
                    moved_end: pt(v.moved_end),
                    becomes: v.becomes.to_string(),
                })
                .collect(),
        })
    }
}

/// The accent presets and consonant knobs.
#[wasm_bindgen]
pub fn catalog() -> CatalogDto {
    CatalogDto {
        accents: accents::PRESETS
            .iter()
            .map(|&(name, description)| PresetDto {
                name: name.into(),
                description: description.into(),
            })
            .collect(),
        knobs: knobs::CONSONANT_KNOBS
            .iter()
            .map(|k| k.to_string())
            .collect(),
    }
}

/// Every knob's default.
#[wasm_bindgen]
pub fn defaults() -> DefaultsDto {
    let search = SearchConfig::default();
    let mishear = MishearConfig::default();
    DefaultsDto {
        lexicon: LexiconOptions {
            vocabulary: 50_000,
            all_pronunciations: true,
            common_words: true,
            bigram_prior: BIGRAM_PRIOR,
        },
        search: SearchDto {
            results: mishear.search.results,
            within: None,
            lm_weight: search.lm_weight,
            word_penalty: search.word_penalty,
            bigrams: search.bigrams,
            shortlist: search.shortlist,
            distinct_sounds: search.distinct_sounds,
            novelty: mishear.novelty,
            function_share: mishear.function_word_share,
            max_bound: mishear.max_bound,
        },
        cost: CostConfig::default().into(),
    }
}
