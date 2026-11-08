//! The words a search may produce: CMUdict pronunciations of the most
//! frequent English words, in a trie, each with a language-model cost.

use std::collections::HashMap;
use std::path::Path;

use mime_reader::PronouncingDictionary;

use crate::bigrams::{BigramTable, dequantize};
use crate::phone::Ph;
use std::sync::Arc;

/// A word id: an index into [`Lexicon::words`].
pub type WordId = u32;

#[derive(Default)]
struct Node {
    /// Sorted by phoneme.
    children: Vec<(Ph, u32)>,
    /// Words whose pronunciation ends here.
    words: Vec<WordId>,
}

/// A pronunciation trie over a frequency-ranked vocabulary.
pub struct Lexicon {
    words: Vec<String>,
    /// `−ln P(word)` under a unigram model.
    surprisal: Vec<f32>,
    nodes: Vec<Node>,
    index: HashMap<String, WordId>,
    max_depth: usize,
    bigrams: Option<Bigrams>,
    /// `sound[w]`: an id shared by every word whose first pronunciation is
    /// the same ("there", "their", "they're").
    sound: Vec<u32>,
    first: Vec<Vec<Ph>>,
    /// The words sharing each [`stem`].
    by_stem: HashMap<String, Vec<WordId>>,
    /// The words with each sound (see [`Self::sound`]).
    by_sound: Vec<Vec<WordId>>,
}

/// A word's stem, by Snowball's English stemmer (Porter2): "fly", "flies"
/// and "flying" all give "fli". Irregular forms keep their own ("flew").
pub fn stem(word: &str) -> String {
    use rust_stemmers::{Algorithm, Stemmer};
    Stemmer::create(Algorithm::English)
        .stem(&word.to_lowercase())
        .into_owned()
}

/// The bigram model over this lexicon: a [`BigramTable`] seen through the
/// lexicon's words, smoothed toward the unigram frequencies:
/// `P(w | v) = (count(v, w) + (μ + r(v))·P(w)) / (count(v) + μ)`, so a pair
/// seen often is likely, a rarely seen left word says little, and the mass
/// of pairs left out of the table (`r(v)`: pruned, or with words outside
/// the lexicon) is spread by `P(w)` rather than lost.
struct Bigrams {
    table: Arc<BigramTable>,
    /// The smoothing strength `μ`: how many words of evidence the unigram
    /// prior is worth.
    prior: f32,
    /// Each lexicon word's index in the table (`u32::MAX`: not there).
    in_table: Vec<u32>,
    /// `count(v)`: how often `v` is followed by any word.
    totals: Vec<f32>,
    /// `μ + r(v)`: the prior's weight after `v`.
    backoff: Vec<f32>,
    /// The cheapest `−ln P(w | ·)` over every context, the start included.
    best: Vec<f32>,
}

/// The default smoothing strength `μ`, chosen for the Google Books counts:
/// with their billions of words, pairs need a strong prior to say little.
pub const BIGRAM_PRIOR: f32 = 100_000.0;

/// A word with its Zipf frequency (log10 occurrences per billion words).
pub struct Frequency {
    pub word: String,
    pub zipf: f32,
}

/// Reads `word<TAB>zipf` lines, most frequent first.
pub fn read_frequencies(path: &Path) -> Result<Vec<Frequency>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_frequencies(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parses `word<TAB>zipf` lines, most frequent first.
pub fn parse_frequencies(text: &str) -> Result<Vec<Frequency>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (word, zipf) = line
                .split_once('\t')
                .ok_or_else(|| format!("malformed line {line:?}"))?;
            let zipf = zipf.trim().parse().map_err(|e| format!("{line:?}: {e}"))?;
            Ok(Frequency {
                word: word.to_string(),
                zipf,
            })
        })
        .collect()
}

/// Real words: those a word list such as ENABLE has (it lists every
/// inflected form, and no names, brands or abbreviations), and contractions
/// and possessives of them: "it's", "don't", "people's", but not "america's"
/// or "o'brien".
pub struct CommonWords {
    words: std::collections::HashSet<String>,
}

/// What follows the apostrophe of a contraction or possessive: "it's",
/// "you're", "i've", "we'll", "she'd", "i'm", "don't".
const CLITICS: [&str; 7] = ["s", "re", "ve", "ll", "d", "m", "t"];

/// Negations whose base isn't a word: "can't", "won't", "shan't", "ain't".
const NEGATED: [&str; 4] = ["ca", "wo", "sha", "ai"];

/// Contractions with parts that aren't words.
const CONTRACTIONS: [&str; 4] = ["o'clock", "c'mon", "y'know", "y'all"];

impl CommonWords {
    /// Reads a word list, one word per line; capitalized entries (proper
    /// nouns, in lists that have them) are ignored.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::parse(&text))
    }

    /// [`Self::read`] from the list's text.
    pub fn parse(text: &str) -> Self {
        Self {
            words: text
                .lines()
                .map(str::trim)
                .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_lowercase()))
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn contains(&self, word: &str) -> bool {
        if word.chars().count() == 1 {
            return matches!(word, "a" | "i" | "o");
        }
        if !word.contains('\'') {
            return self.words.contains(word);
        }
        if CONTRACTIONS.contains(&word) {
            return true;
        }
        if let Some(base) = word.strip_suffix("n't") {
            return self.words.contains(base) || NEGATED.contains(&base);
        }
        // "it's", "people's", "i'm", "'em": a word, then clitics.
        word.split('\'').enumerate().all(|(k, part)| {
            part.is_empty()
                || part == "i"
                || self.words.contains(part)
                || (k > 0 && CLITICS.contains(&part))
        })
    }
}

impl Lexicon {
    /// The `max_words` most frequent words that CMUdict can pronounce, that
    /// are spelled with letters and apostrophes only, and that `keep`
    /// accepts — with every CMUdict pronunciation, or only the first.
    pub fn build<D: PronouncingDictionary>(
        dict: &D,
        frequencies: &[Frequency],
        max_words: usize,
        all_pronunciations: bool,
        keep: impl Fn(&str) -> bool,
    ) -> Self {
        let mut lexicon = Self {
            words: Vec::new(),
            surprisal: Vec::new(),
            nodes: vec![Node::default()],
            index: HashMap::new(),
            max_depth: 0,
            bigrams: None,
            sound: Vec::new(),
            first: Vec::new(),
            by_stem: HashMap::new(),
            by_sound: Vec::new(),
        };
        let mut sounds: HashMap<Vec<Ph>, u32> = HashMap::new();
        for frequency in frequencies {
            if lexicon.words.len() >= max_words {
                break;
            }
            let word = frequency.word.as_str();
            let spelled_plainly = word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'')
                && word.chars().any(|c| c.is_ascii_alphabetic());
            if !spelled_plainly || lexicon.index.contains_key(word) || !keep(word) {
                continue;
            }
            let Some(pronunciations) = dict.lookup(word) else {
                continue;
            };
            let id = lexicon.words.len() as WordId;
            lexicon.words.push(word.to_string());
            // P = 10^(zipf − 9) occurrences per word of text.
            lexicon
                .surprisal
                .push((9.0 - frequency.zipf) * std::f32::consts::LN_10);
            lexicon.index.insert(word.to_string(), id);
            lexicon.by_stem.entry(stem(word)).or_default().push(id);
            let first: Vec<Ph> = pronunciations[0].iter().map(Ph::from_phoneme).collect();
            let next = sounds.len() as u32;
            lexicon
                .sound
                .push(*sounds.entry(first.clone()).or_insert(next));
            let sound = lexicon.sound[id as usize] as usize;
            if sound == lexicon.by_sound.len() {
                lexicon.by_sound.push(Vec::new());
            }
            lexicon.by_sound[sound].push(id);
            lexicon.first.push(first);
            let mut seen: Vec<Vec<Ph>> = Vec::new();
            let count = if all_pronunciations {
                pronunciations.len()
            } else {
                1
            };
            for pronunciation in &pronunciations[..count] {
                let phonemes: Vec<Ph> = pronunciation.iter().map(Ph::from_phoneme).collect();
                if !seen.contains(&phonemes) {
                    lexicon.insert(&phonemes, id);
                    seen.push(phonemes);
                }
            }
        }
        lexicon
    }

    fn insert(&mut self, phonemes: &[Ph], word: WordId) {
        let mut node = 0usize;
        for &p in phonemes {
            let children = &self.nodes[node].children;
            node = match children.binary_search_by_key(&p, |&(q, _)| q) {
                Ok(k) => children[k].1 as usize,
                Err(k) => {
                    let child = self.nodes.len();
                    self.nodes.push(Node::default());
                    self.nodes[node].children.insert(k, (p, child as u32));
                    child
                }
            };
        }
        self.nodes[node].words.push(word);
        self.max_depth = self.max_depth.max(phonemes.len());
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The word's sound class: equal for homophones (by first
    /// pronunciation).
    pub fn sound(&self, id: WordId) -> u32 {
        self.sound[id as usize]
    }

    /// The word's first pronunciation.
    pub fn first_pronunciation(&self, id: WordId) -> &[Ph] {
        &self.first[id as usize]
    }

    pub fn word(&self, id: WordId) -> &str {
        &self.words[id as usize]
    }

    pub fn id(&self, word: &str) -> Option<WordId> {
        self.index.get(&word.to_lowercase()).copied()
    }

    /// Every word that sounds like `id` (by first pronunciation), `id`
    /// itself included: "there", "their", "they're".
    pub fn homophones(&self, id: WordId) -> &[WordId] {
        &self.by_sound[self.sound(id) as usize]
    }

    /// Every word with `word`'s [`stem`], `word` itself included.
    pub fn same_stem(&self, word: &str) -> &[WordId] {
        self.by_stem.get(&stem(word)).map_or(&[], Vec::as_slice)
    }

    /// `−ln P(word)`: about 3 for "the", 8 for "nice", 15 for "aardvark".
    pub fn surprisal(&self, id: WordId) -> f32 {
        self.surprisal[id as usize]
    }

    /// Reads word-pair counts (a [`BigramTable`] file, text or packed) for
    /// [`Self::surprisal_after`], smoothed with strength `prior` (μ; see
    /// [`BIGRAM_PRIOR`]).
    pub fn load_bigrams(&mut self, path: &Path, prior: f32) -> Result<(), String> {
        self.attach_bigrams(Arc::new(BigramTable::read(path)?), prior);
        Ok(())
    }

    /// [`Self::load_bigrams`] from the text format.
    pub fn parse_bigrams(&mut self, text: &str, prior: f32) -> Result<(), String> {
        self.attach_bigrams(Arc::new(BigramTable::parse_text(text)?), prior);
        Ok(())
    }

    /// Scores words by the previous word too, with `table`'s counts
    /// smoothed with strength `prior`.
    pub fn attach_bigrams(&mut self, table: Arc<BigramTable>, prior: f32) {
        let n = self.len();
        let in_table: Vec<u32> = self
            .words
            .iter()
            .map(|w| table.id(w).unwrap_or(u32::MAX))
            .collect();
        let mut in_lexicon = vec![u32::MAX; table.words().len()];
        for (v, &t) in in_table.iter().enumerate() {
            if t != u32::MAX {
                in_lexicon[t as usize] = v as u32;
            }
        }
        let totals: Vec<f32> = in_table
            .iter()
            .map(|&t| {
                if t == u32::MAX {
                    0.0
                } else {
                    table.total(t) as f32
                }
            })
            .collect();
        let mut backoff = vec![prior; n];
        for (v, &t) in in_table.iter().enumerate() {
            if t == u32::MAX {
                continue;
            }
            let (rights, counts) = table.row(t);
            let listed: f64 = rights
                .iter()
                .zip(counts)
                .filter(|&(&w, _)| in_lexicon[w as usize] != u32::MAX)
                .map(|(_, &q)| f64::from(dequantize(q)))
                .sum();
            backoff[v] = prior + (f64::from(totals[v]) - listed).max(0.0) as f32;
        }
        // Unseen pairs get at most `P(w)`, since `r(v) ≤ count(v)`: the
        // cheapest context is the unigram or a listed pair.
        let mut best: Vec<f32> = self.surprisal.clone();
        for (v, &t) in in_table.iter().enumerate() {
            if t == u32::MAX {
                continue;
            }
            let (rights, counts) = table.row(t);
            for (&w, &q) in rights.iter().zip(counts) {
                let w = in_lexicon[w as usize];
                if w == u32::MAX {
                    continue;
                }
                let p = (dequantize(q) + backoff[v] * (-self.surprisal[w as usize]).exp())
                    / (totals[v] + prior);
                best[w as usize] = best[w as usize].min(-p.ln());
            }
        }
        self.bigrams = Some(Bigrams {
            table,
            prior,
            in_table,
            totals,
            backoff,
            best,
        });
    }

    /// The word-pair counts in use, and their smoothing strength.
    pub fn bigram_table(&self) -> Option<(Arc<BigramTable>, f32)> {
        self.bigrams.as_ref().map(|b| (b.table.clone(), b.prior))
    }

    pub fn has_bigrams(&self) -> bool {
        self.bigrams.is_some()
    }

    /// `−ln P(word | previous)`: the bigram model when loaded (and there is
    /// a previous word), else [`Self::surprisal`].
    pub fn surprisal_after(&self, previous: Option<WordId>, word: WordId) -> f32 {
        let unigram = self.surprisal[word as usize];
        match (&self.bigrams, previous) {
            (Some(b), Some(v)) => {
                let (tv, tw) = (b.in_table[v as usize], b.in_table[word as usize]);
                let count = if tv == u32::MAX || tw == u32::MAX {
                    0.0
                } else {
                    b.table.count(tv, tw).unwrap_or(0.0)
                };
                let p = (count + b.backoff[v as usize] * (-unigram).exp())
                    / (b.totals[v as usize] + b.prior);
                -p.ln()
            }
            _ => unigram,
        }
    }

    /// The cheapest [`Self::surprisal_after`] over every context: a lower
    /// bound the search prunes with.
    pub fn best_surprisal(&self, word: WordId) -> f32 {
        match &self.bigrams {
            Some(b) => b.best[word as usize],
            None => self.surprisal[word as usize],
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    pub(crate) fn children(&self, node: usize) -> &[(Ph, u32)] {
        &self.nodes[node].children
    }

    pub(crate) fn words_at(&self, node: usize) -> &[WordId] {
        &self.nodes[node].words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_words_keep_listed_words_and_their_contractions() {
        let common = CommonWords::parse(
            "boy\nboys\nrunning\nlove\ncities\nit\ndo\nis\npeople\nem\ncause\nma\nam\nTexas\n",
        );
        for word in [
            "boy", "boys", "running", "cities", "a", "i", "o", "boy's", "people's", "it's", "i'm",
            "don't", "isn't", "can't", "won't", "'em", "'cause", "ma'am", "o'clock",
        ] {
            assert!(common.contains(word), "{word}");
        }
        // Listed forms only (no guessing at "loved"), and no names.
        for word in [
            "loved",
            "boyd",
            "b",
            "texas",
            "james's",
            "o'brien",
            "america's",
            "doesn't",
        ] {
            assert!(!common.contains(word), "{word}");
        }
    }
}
