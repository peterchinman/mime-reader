//! The mishearing engine: read a line, deform its sounds, and find the word
//! sequences that best match the result — preferring ones that share few
//! words with the original, so the output is a mishearing rather than the
//! line itself.

use std::collections::{HashMap, HashSet};
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
    /// The share of `novelty` (and of `root`) charged for a shared function
    /// word.
    pub function_word_share: f32,
    /// Extra cost per output word sharing a stem with a word of the line
    /// ("flying" for "fly", "flies" for "fly"): the same word included, so
    /// it adds to `novelty` there.
    pub root: f32,
    /// Reward per word boundary of the line that a hearing cuts across
    /// ("this guy" for "the sky" cuts "the|sky"). Charged instead as a cost
    /// on each boundary a hearing keeps: every hearing keeps or cuts each
    /// boundary, so the ranking is the same, costs stay non-negative, and
    /// the search stays exact.
    pub crossing: f32,
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
            root: 0.0,
            crossing: 0.0,
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
    /// How many of its other words share a stem with one of the line's.
    pub related: usize,
    /// How many of the line's word boundaries it keeps.
    pub kept: usize,
}

#[derive(Debug, Clone)]
pub struct Mishearing {
    pub reading: Reading,
    pub deformed: Vec<Vec<Ph>>,
    /// The line's word boundaries, as positions in the deformed input
    /// (between words, so neither 0 nor its length).
    pub boundaries: Vec<usize>,
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
        let share = |w: Option<WordId>| match w {
            Some(w) if w < FUNCTION_WORDS => config.function_word_share,
            _ => 1.0,
        };
        let mut extra: HashMap<WordId, f32> = search.extra.clone();
        for &w in &originals {
            *extra.entry(w).or_default() += config.novelty * share(Some(w));
        }
        // Words sharing a stem with the line's, charged once each, at the
        // largest share among the line words they relate to.
        let mut root: HashMap<WordId, f32> = HashMap::new();
        for word in &reading.words {
            let charge = config.root * share(self.lexicon.id(word));
            for &w in self.lexicon.same_stem(word) {
                let entry = root.entry(w).or_default();
                *entry = entry.max(charge);
            }
        }
        let related: HashSet<WordId> = root.keys().copied().collect();
        for (w, charge) in root {
            *extra.entry(w).or_default() += charge;
        }
        search.extra = extra;
        let mut boundaries: Vec<usize> = deformed
            .iter()
            .scan(0, |at, word| {
                *at += word.len();
                Some(*at)
            })
            .filter(|&p| p > 0 && p < input.len())
            .collect();
        boundaries.dedup();
        if config.crossing > 0.0 {
            let mut cost = vec![0.0; input.len()];
            for &p in &boundaries {
                cost[p] += config.crossing;
            }
            search.boundary_cost = cost;
        }
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
                related: m
                    .words
                    .iter()
                    .filter(|w| !originals.contains(w) && related.contains(w))
                    .count(),
                kept: m
                    .spans
                    .iter()
                    .filter(|span| boundaries.contains(&span.start))
                    .count(),
            })
            .collect();
        Ok(Mishearing {
            reading,
            deformed,
            boundaries,
            heard,
            stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "fly in", over a lexicon where "flying" sounds nearly the same.
    fn fly_in() -> Engine {
        let dict = CmuDictionary::from_bytes(
            b"FLY  F L AY1\nIN  IH1 N\nFLYING  F L AY1 IH0 NG\nFLEW  F L UW1\nLINE  L AY1 N\nFLEA  F L IY1\nINN  IH1 N\n",
        )
        .unwrap();
        let frequencies: Vec<Frequency> = [
            ("in", 7.0),
            ("fly", 4.5),
            ("flying", 4.5),
            ("line", 4.5),
            ("flew", 4.0),
            ("flea", 3.0),
            ("inn", 3.0),
        ]
        .iter()
        .map(|&(word, zipf)| Frequency {
            word: word.into(),
            zipf,
        })
        .collect();
        Engine::new(dict, &frequencies, 100, true, None)
    }

    fn heard(engine: &Engine, config: &MishearConfig) -> Vec<Heard> {
        engine
            .mishear("fly in", &Deformation::new(""), config)
            .unwrap()
            .heard
    }

    fn config(root: f32, crossing: f32) -> MishearConfig {
        let mut config = MishearConfig {
            root,
            crossing,
            function_word_share: 1.0,
            novelty: 0.0,
            ..MishearConfig::default()
        };
        config.search.bigrams = false;
        config.search.distinct_sounds = false;
        config.search.results = 20;
        config
    }

    fn cost_of(heard: &[Heard], text: &str) -> f32 {
        heard.iter().find(|h| h.text == text).unwrap().cost
    }

    #[test]
    fn the_root_penalty_charges_words_sharing_a_stem() {
        let engine = fly_in();
        let plain = heard(&engine, &config(0.0, 0.0));
        let rooted = heard(&engine, &config(1.5, 0.0));
        // "flying" shares "fly"'s stem; "flew" is irregular, so it doesn't.
        let flying = plain.iter().find(|h| h.text == "flying").unwrap();
        assert_eq!((flying.shared, flying.related), (0, 1));
        let diff = |t: &str| cost_of(&rooted, t) - cost_of(&plain, t);
        assert!((diff("flying") - 1.5).abs() < 1e-5);
        assert!(diff("flew inn").abs() < 1e-5);
        // Each "fly" or "in" is charged: the same word shares its own stem.
        assert!((diff("fly inn") - 1.5).abs() < 1e-5);
        assert!((diff("fly line") - 1.5).abs() < 1e-5);
        // Once the charge outweighs the sound, "flying" gives way to the
        // nearest hearing sharing no stem with the line.
        assert_eq!(plain[0].text, "flying");
        assert_eq!(rooted[0].text, "flea inn");
    }

    #[test]
    fn the_crossing_reward_charges_kept_boundaries() {
        let engine = fly_in();
        let plain = heard(&engine, &config(0.0, 0.0));
        let crossing = heard(&engine, &config(0.0, 0.5));
        let mishearing = engine
            .mishear("fly in", &Deformation::new(""), &config(0.0, 0.5))
            .unwrap();
        assert_eq!(mishearing.boundaries, vec![3]);
        // "flea inn" keeps "fly|in"; "flying" and "flea line" ("F L IY1 |
        // L AY1 N" against "F L AY1 | IH1 N") cut across it.
        let kept = |t: &str| crossing.iter().find(|h| h.text == t).unwrap().kept;
        assert_eq!((kept("flea inn"), kept("flying")), (1, 0));
        let diff = |t: &str| cost_of(&crossing, t) - cost_of(&plain, t);
        assert!((diff("flea inn") - 0.5).abs() < 1e-5);
        assert!(diff("flying").abs() < 1e-5);
    }
}
