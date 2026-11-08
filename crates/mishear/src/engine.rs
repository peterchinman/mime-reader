//! The mishearing engine: read a line, deform its sounds, and find the word
//! sequences that best match the result — preferring ones that share few
//! words with the original, so the output is a mishearing rather than the
//! line itself.

use std::collections::HashMap;
use std::path::Path;

use mime_reader::line::WordData;
use mime_reader::{
    CmuDictionary, ConfiguredDictionary, Line, PronouncingDictionary, PronunciationMode,
};

use crate::cost::CostModel;
use crate::deform::Deformation;
use crate::lexicon::{CommonWords, Frequency, Lexicon, WordId, read_frequencies};
use crate::phone::Ph;
use crate::search::{Match, SearchConfig, SearchStats, best_matches};

/// Words this frequent (by rank) count as function words: sharing "the"
/// with the original line costs less than sharing "river".
const FUNCTION_WORDS: WordId = 150;

#[derive(Debug, Clone)]
pub struct MishearConfig {
    pub search: SearchConfig,
    /// Extra cost per output word that the original line contains.
    pub novelty: f32,
    /// The share of `novelty` charged for a shared function word.
    pub function_word_share: f32,
    /// How far [`best_matches`] may raise the bound.
    pub max_bound: f32,
}

impl Default for MishearConfig {
    fn default() -> Self {
        Self {
            search: SearchConfig {
                results: 8,
                bound: 2.0,
                ..SearchConfig::default()
            },
            novelty: 1.0,
            function_word_share: 0.2,
            max_bound: 30.0,
        }
    }
}

/// A line, read.
#[derive(Debug, Clone)]
pub struct Reading {
    pub words: Vec<String>,
    /// Each word's first pronunciation.
    pub phonemes: Vec<Vec<Ph>>,
}

/// One mishearing.
#[derive(Debug, Clone)]
pub struct Heard {
    pub text: String,
    pub words: Vec<String>,
    /// The (deformed) input span each word covers.
    pub spans: Vec<std::ops::Range<usize>>,
    pub cost: f32,
    pub phonetic: f32,
    /// How many of its words the original contains.
    pub shared: usize,
}

#[derive(Debug, Clone)]
pub struct Mishearing {
    pub reading: Reading,
    pub deformed: Vec<Vec<Ph>>,
    pub heard: Vec<Heard>,
    pub stats: SearchStats,
}

/// A dictionary, a lexicon and a cost model, loaded once.
pub struct Engine {
    dict: ConfiguredDictionary<CmuDictionary>,
    pub lexicon: Lexicon,
    pub cost: CostModel,
}

impl Engine {
    /// Loads CMUdict from `cmudict` and the frequency list from
    /// `frequencies`, keeping the `vocabulary` most frequent words — only
    /// those in `common` when given — with every pronunciation or only the
    /// first.
    pub fn load(
        cmudict: &Path,
        frequencies: &Path,
        vocabulary: usize,
        all_pronunciations: bool,
        common: Option<&CommonWords>,
    ) -> Result<Self, String> {
        let dict =
            CmuDictionary::load(cmudict).map_err(|e| format!("{}: {e}", cmudict.display()))?;
        Ok(Self::new(
            dict,
            &read_frequencies(frequencies)?,
            vocabulary,
            all_pronunciations,
            common,
        ))
    }

    /// An engine over `dict`, keeping the `vocabulary` most frequent of
    /// `frequencies` (only those in `common` when given).
    pub fn new(
        dict: CmuDictionary,
        frequencies: &[Frequency],
        vocabulary: usize,
        all_pronunciations: bool,
        common: Option<&CommonWords>,
    ) -> Self {
        let lexicon = Lexicon::build(&dict, frequencies, vocabulary, all_pronunciations, |w| {
            common.is_none_or(|c| c.contains(w))
        });
        Self::with_lexicon(dict, lexicon)
    }

    /// Rebuilds the lexicon (dropping any bigrams: reload them after).
    pub fn rebuild_lexicon(
        &mut self,
        frequencies: &[Frequency],
        vocabulary: usize,
        all_pronunciations: bool,
        common: Option<&CommonWords>,
    ) {
        self.lexicon = Lexicon::build(
            &self.dict,
            frequencies,
            vocabulary,
            all_pronunciations,
            |w| common.is_none_or(|c| c.contains(w)),
        );
    }

    /// An engine over an already-built lexicon.
    pub fn with_lexicon(dict: CmuDictionary, lexicon: Lexicon) -> Self {
        // Reading text may derive pronunciations for inflections CMUdict
        // lacks; the lexicon sticks to its headwords (raw lookup).
        Self {
            dict: dict.with_pronunciation_mode(PronunciationMode::AllowDerived),
            lexicon,
            cost: CostModel::default(),
        }
    }

    /// The line's words and their first pronunciations.
    pub fn read(&self, text: &str) -> Result<Reading, String> {
        let line = Line::new(text, &self.dict);
        let mut reading = Reading {
            words: Vec::new(),
            phonemes: Vec::new(),
        };
        for word in &line.words {
            match &word.data {
                WordData::Known { pronunciations, .. } => {
                    reading.words.push(word.word.to_lowercase());
                    reading.phonemes.push(
                        pronunciations[0]
                            .phonemes
                            .iter()
                            .map(Ph::from_phoneme)
                            .collect(),
                    );
                }
                WordData::Unknown => return Err(format!("unknown word {:?}", word.word)),
            }
        }
        if reading.words.is_empty() {
            return Err("no words".into());
        }
        Ok(reading)
    }

    /// The best word sequences for `phonemes` (see [`best_matches`]).
    pub fn hear(
        &self,
        phonemes: &[Ph],
        config: &SearchConfig,
        max_bound: f32,
    ) -> (Vec<Match>, SearchStats) {
        best_matches(phonemes, &self.lexicon, &self.cost, config, 1.5, max_bound)
    }

    /// Reads `text`, deforms it, and hears it back.
    pub fn mishear(
        &self,
        text: &str,
        deformation: &Deformation,
        config: &MishearConfig,
    ) -> Result<Mishearing, String> {
        let reading = self.read(text)?;
        let deformed = deformation.apply_spelled(&reading.phonemes, &reading.words);
        let input: Vec<Ph> = deformed.iter().flatten().copied().collect();
        let mut search = config.search.clone();
        let originals: Vec<WordId> = reading
            .words
            .iter()
            .filter_map(|w| self.lexicon.id(w))
            .collect();
        let mut extra: HashMap<WordId, f32> = search.extra.clone();
        for &w in &originals {
            let share = if w < FUNCTION_WORDS {
                config.function_word_share
            } else {
                1.0
            };
            *extra.entry(w).or_default() += config.novelty * share;
        }
        search.extra = extra;
        let (matches, stats) = self.hear(&input, &search, config.max_bound);
        // The line itself is not a mishearing (its homophone respellings,
        // "hark the harold angels", may well be).
        let original_words: Vec<Option<WordId>> =
            reading.words.iter().map(|w| self.lexicon.id(w)).collect();
        let heard = matches
            .iter()
            .filter(|m| {
                m.words.len() != original_words.len()
                    || m.words
                        .iter()
                        .zip(&original_words)
                        .any(|(w, o)| Some(*w) != *o)
            })
            .map(|m| Heard {
                text: m.text(&self.lexicon),
                words: m
                    .words
                    .iter()
                    .map(|&w| self.lexicon.word(w).to_string())
                    .collect(),
                spans: m.spans.clone(),
                cost: m.cost,
                phonetic: m.phonetic,
                shared: m.words.iter().filter(|w| originals.contains(w)).count(),
            })
            .collect();
        Ok(Mishearing {
            reading,
            deformed,
            heard,
            stats,
        })
    }
}
