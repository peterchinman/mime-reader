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

use crate::clock::Stopwatch;
use crate::cost::CostModel;
use crate::deform::Deformation;
use crate::lexicon::{CommonWords, Frequency, Lexicon, WordId, read_frequencies};
use crate::phone::Ph;
use crate::search::{
    Lattice, Match, SearchConfig, SearchStats, best_matches, beyond, cheapest, lattice, next_bound,
    paths, shortlist,
};

/// Words this frequent (by rank) count as function words: sharing "the"
/// with the original line costs less than sharing "river".
const FUNCTION_WORDS: WordId = 150;

/// How many hearings beyond those taken each pass 2 of
/// [`Engine::hear_varied`] returns, to pick the next ones from.
const POOL: usize = 24;

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
    /// Extra cost per output word that an earlier hearing used, or a
    /// homophone of one ("cents" after "sense"), with function words at
    /// `function_word_share`; with `root`, words sharing their stems (and
    /// those words' homophones) too.
    /// Each hearing is then the best one given those before it, so the list
    /// varies. 0: the plain top `results`, from one search.
    pub variety: f32,
    /// How far [`best_matches`] may raise the bound.
    pub max_bound: f32,
}

/// How hearings are kept apart from one another (see
/// [`MishearConfig::variety`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct Variety {
    pub variety: f32,
    pub root: f32,
    pub function_word_share: f32,
}

impl Default for MishearConfig {
    fn default() -> Self {
        Self {
            search: SearchConfig {
                results: 4,
                bound: 2.0,
                ..SearchConfig::default()
            },
            novelty: 0.9,
            function_word_share: 0.2,
            root: 0.2,
            crossing: 0.2,
            variety: 1.0,
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
    /// How many of its words an earlier hearing used (or a homophone).
    pub reused: usize,
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

    /// The best `config.results` hearings of `input`, best first, leaving
    /// out those `exclude` rejects, each with how many of its words an
    /// earlier one used (homophones included).
    ///
    /// With `variety.variety` > 0, each is the best hearing given the ones
    /// before it: their words and homophones cost `variety` more, and words
    /// sharing their stems (and those words' homophones) `root` more, with
    /// function words at
    /// `function_word_share`. Penalties only add cost, so one pass-1
    /// lattice serves every hearing, as long as it keeps, per span, enough
    /// words to cover the penalized ones; it is rebuilt when a hearing
    /// needs a higher bound or more words.
    pub fn hear_varied(
        &self,
        input: &[Ph],
        config: &SearchConfig,
        variety: Variety,
        max_bound: f32,
        exclude: impl Fn(&Match) -> bool,
    ) -> (Vec<(Match, usize)>, SearchStats) {
        let start = Stopwatch::start();
        let wanted = config.results;
        let sounds =
            |m: &Match| -> Vec<u32> { m.words.iter().map(|&w| self.lexicon.sound(w)).collect() };
        let mut chosen: Vec<(Match, usize)> = Vec::new();
        let mut seen: Vec<Vec<u32>> = Vec::new();
        let mut used: HashSet<WordId> = HashSet::new();
        // Keeps `m`, counting the words earlier hearings used.
        let take = |m: Match,
                    used: &mut HashSet<WordId>,
                    seen: &mut Vec<Vec<u32>>,
                    chosen: &mut Vec<(Match, usize)>| {
            let reused = m.words.iter().filter(|w| used.contains(w)).count();
            for &w in &m.words {
                used.extend(self.lexicon.homophones(w));
            }
            seen.push(sounds(&m));
            chosen.push((m, reused));
        };

        if variety.variety <= 0.0 {
            let mut search = config.clone();
            search.results = wanted + 1; // room for an excluded one
            let (matches, stats) = self.hear(input, &search, max_bound);
            for m in matches.into_iter().filter(|m| !exclude(m)).take(wanted) {
                take(m, &mut used, &mut seen, &mut chosen);
            }
            return (chosen, stats);
        }

        let mut stats = SearchStats::default();
        let mut penalty: HashMap<WordId, f32> = HashMap::new();
        let mut stem_penalty: HashMap<WordId, f32> = HashMap::new();
        let charges = |penalty: &HashMap<WordId, f32>, stems: &HashMap<WordId, f32>| {
            let mut all = penalty.clone();
            for (&w, &charge) in stems {
                *all.entry(w).or_default() += charge;
            }
            all
        };
        let charge_of = |m: &Match, charges: &HashMap<WordId, f32>| -> f32 {
            m.words
                .iter()
                .map(|w| charges.get(w).copied().unwrap_or(0.0))
                .sum()
        };
        let mut search = config.clone();
        let mut bound = config.bound;
        let mut built: Option<Lattice> = None;
        // Beyond the hearings already taken, how many more each pass 2
        // returns: the pool later hearings are picked from.
        let mut extra_pool = POOL;
        'hearings: while seen.len() < wanted {
            let at_call = charges(&penalty, &stem_penalty);
            let ask = seen.len() + 1 + extra_pool;
            search.results = ask;
            let keep = shortlist(&self.lexicon, &search, at_call.len());
            if built
                .as_ref()
                .is_none_or(|l| l.bound < bound || l.keep < keep)
            {
                // Room, too, for the words the hearings still to come will
                // penalize, so they needn't rebuild it.
                let per_hearing = if variety.root > 0.0 { 24 } else { 12 };
                search.bound = bound;
                let l = lattice(
                    input,
                    &self.lexicon,
                    &self.cost,
                    &search,
                    keep + per_hearing * (wanted - seen.len()),
                );
                stats.nodes += l.nodes;
                stats.rounds += 1;
                stats.edges = l.edge_count;
                stats.bound = l.bound;
                built = Some(l);
            }
            let l = built.as_ref().expect("built above");
            search.bound = l.bound;
            // Nothing left within the bound: skip pass 2, and raise the
            // bound to where the cheapest hearing lies.
            let floor = cheapest(l, &search, &at_call);
            if beyond(floor, l.bound) {
                if bound >= max_bound {
                    break 'hearings;
                }
                bound = next_bound(l.bound, floor, 1.5, max_bound);
                continue;
            }
            let pool = paths(l, &self.lexicon, &search, &at_call);
            // Every hearing outside the pool costs at least as much as its
            // last member did (or, when the pool holds everything within
            // the bound, more than the bound), and penalties only add.
            let full = pool.len() == ask;
            let floor = if full {
                pool.last().map_or(f32::INFINITY, |m| m.cost)
            } else {
                l.bound
            };
            let mut taken_here = 0;
            while seen.len() < wanted {
                let now = charges(&penalty, &stem_penalty);
                let best = pool
                    .iter()
                    .filter(|m| !exclude(m) && !seen.contains(&sounds(m)))
                    .map(|m| (m, m.cost - charge_of(m, &at_call) + charge_of(m, &now)))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                match best {
                    // The best of the pool, and provably of all.
                    Some((m, cost)) if cost <= floor => {
                        let mut m = m.clone();
                        m.cost = cost;
                        for &w in &m.words {
                            let share = if w < FUNCTION_WORDS {
                                variety.function_word_share
                            } else {
                                1.0
                            };
                            for &h in self.lexicon.homophones(w) {
                                let entry = penalty.entry(h).or_default();
                                *entry = entry.max(variety.variety * share);
                            }
                            if variety.root > 0.0 {
                                // Their homophones too, so every spelling of a
                                // sound pays alike: pass 2 keeps one spelling
                                // per sound, and the pool's rescoring relies on
                                // that one staying the cheapest.
                                for &s in self.lexicon.same_stem(self.lexicon.word(w)) {
                                    for &h in self.lexicon.homophones(s) {
                                        let entry = stem_penalty.entry(h).or_default();
                                        *entry = entry.max(variety.root * share);
                                    }
                                }
                            }
                        }
                        take(m, &mut used, &mut seen, &mut chosen);
                        taken_here += 1;
                    }
                    // Something outside the pool may be better: search again.
                    Some(_) => break,
                    None if full => break,
                    None => {
                        // Everything within the bound is taken.
                        if bound >= max_bound {
                            break 'hearings;
                        }
                        bound = next_bound(bound, floor, 1.5, max_bound);
                        break;
                    }
                }
            }
            // A full pool that yielded nothing was all earlier hearings:
            // ask for more next time.
            if taken_here == 0 && full {
                extra_pool *= 2;
            }
        }
        stats.elapsed = start.elapsed();
        (chosen, stats)
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
        // The line itself is not a mishearing (its homophone respellings,
        // "hark the harold angels", may well be).
        let original_words: Vec<Option<WordId>> =
            reading.words.iter().map(|w| self.lexicon.id(w)).collect();
        let is_line = |m: &Match| {
            m.words.len() == original_words.len()
                && m.words
                    .iter()
                    .zip(&original_words)
                    .all(|(w, o)| Some(*w) == *o)
        };
        let variety = Variety {
            variety: config.variety,
            root: config.root,
            function_word_share: config.function_word_share,
        };
        let (matches, stats) =
            self.hear_varied(&input, &search, variety, config.max_bound, is_line);
        let heard = matches
            .iter()
            .map(|(m, reused)| Heard {
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
                reused: *reused,
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
            variety: 0.0,
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

    /// The definition [`Engine::hear_varied`] implements, one full search
    /// per hearing: the best hearing given the penalties of those before.
    fn varied_by_research(
        engine: &Engine,
        input: &[Ph],
        config: &SearchConfig,
        variety: Variety,
    ) -> Vec<(Vec<WordId>, f32)> {
        let mut out: Vec<(Vec<WordId>, f32)> = Vec::new();
        let mut seen: Vec<Vec<u32>> = Vec::new();
        let mut penalty: HashMap<WordId, f32> = HashMap::new();
        let mut stems: HashMap<WordId, f32> = HashMap::new();
        let sounds = |words: &[WordId]| -> Vec<u32> {
            words.iter().map(|&w| engine.lexicon.sound(w)).collect()
        };
        while out.len() < config.results {
            let mut search = config.clone();
            for (&w, &c) in penalty.iter().chain(&stems) {
                *search.extra.entry(w).or_default() += c;
            }
            search.results = out.len() + 30;
            let (matches, _) = engine.hear(input, &search, 60.0);
            let Some(m) = matches
                .into_iter()
                .find(|m| !seen.contains(&sounds(&m.words)))
            else {
                break;
            };
            for &w in &m.words {
                let share = if w < FUNCTION_WORDS {
                    variety.function_word_share
                } else {
                    1.0
                };
                for &h in engine.lexicon.homophones(w) {
                    let e = penalty.entry(h).or_default();
                    *e = e.max(variety.variety * share);
                }
                for &s in engine.lexicon.same_stem(engine.lexicon.word(w)) {
                    for &h in engine.lexicon.homophones(s) {
                        let e = stems.entry(h).or_default();
                        *e = e.max(variety.root * share);
                    }
                }
            }
            seen.push(sounds(&m.words));
            out.push((m.words, m.cost));
        }
        out
    }

    #[test]
    fn variety_matches_a_search_per_hearing() {
        let engine = fly_in();
        for text in [
            "F L AY1 IH1 N",
            "IH1 N F L AY1 IH0 NG",
            "L AY1 N F L IY1 IH1 N",
        ] {
            let input = Ph::parse_all(text).unwrap();
            let settings = [(0.3, 0.0), (1.0, 0.0), (1.0, 0.5), (3.0, 0.0), (0.5, 1.0)];
            let weights = [(0.05, 0.1), (0.005, 0.4), (0.0, 0.0)];
            for ((v, root), (lm_weight, word_penalty)) in settings
                .iter()
                .flat_map(|s| weights.iter().map(move |w| (*s, *w)))
            {
                let variety = Variety {
                    variety: v,
                    root,
                    function_word_share: 0.5,
                };
                let config = SearchConfig {
                    results: 6,
                    bound: 0.5,
                    bigrams: false,
                    lm_weight,
                    word_penalty,
                    ..SearchConfig::default()
                };
                let (fast, _) = engine.hear_varied(&input, &config, variety, 60.0, |_| false);
                let slow = varied_by_research(&engine, &input, &config, variety);
                let fast: Vec<f32> = fast.iter().map(|(m, _)| m.cost).collect();
                let slow: Vec<f32> = slow.iter().map(|(_, c)| *c).collect();
                assert_eq!(fast.len(), slow.len(), "{text} {v} {root}");
                for (a, b) in fast.iter().zip(&slow) {
                    assert!(
                        (a - b).abs() < 1e-4,
                        "{text} {v} {root}: {fast:?} vs {slow:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn variety_keeps_hearings_apart() {
        let engine = fly_in();
        let mut config = config(0.0, 0.0);
        config.search.results = 4;
        config.variety = 0.0;
        let plain = heard(&engine, &config);
        config.variety = 5.0;
        let varied = heard(&engine, &config);
        assert_eq!(plain[0].text, varied[0].text);
        // The plain list repeats words; the varied one doesn't, while it can.
        assert!(plain.iter().skip(1).any(|h| h.reused > 0));
        assert_eq!(varied[1].reused, 0);
        assert!(
            varied
                .iter()
                .zip(&plain)
                .all(|(v, p)| v.cost >= p.cost - 1e-4)
        );
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
