//! The words a search may produce: CMUdict pronunciations of the most
//! frequent English words, in a trie, each with a language-model cost.

use std::collections::HashMap;
use std::path::Path;

use mime_reader::CmuDictionary;

use crate::phone::Ph;

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
}

/// Word-pair counts, smoothed toward the unigram frequencies:
/// `P(w | v) = (count(v, w) + μ·P(w)) / (count(v) + μ)`, so a pair seen often
/// is likely, and a rarely seen left word says little.
struct Bigrams {
    /// The smoothing strength `μ`: how many words of evidence the unigram
    /// prior is worth.
    prior: f32,
    /// `count(v)`: how often `v` is followed by any word.
    totals: Vec<f32>,
    pairs: HashMap<(WordId, WordId), f32>,
    /// The cheapest `−ln P(w | ·)` over every context, the start included.
    best: Vec<f32>,
}

/// The default smoothing strength `μ`.
pub const BIGRAM_PRIOR: f32 = 100.0;

/// A word with its Zipf frequency (log10 occurrences per billion words).
pub struct Frequency {
    pub word: String,
    pub zipf: f32,
}

/// Reads `word<TAB>zipf` lines, most frequent first.
pub fn read_frequencies(path: &Path) -> Result<Vec<Frequency>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (word, zipf) = line
                .split_once('\t')
                .ok_or_else(|| format!("{}: malformed line {line:?}", path.display()))?;
            let zipf = zipf
                .trim()
                .parse()
                .map_err(|e| format!("{}: {line:?}: {e}", path.display()))?;
            Ok(Frequency {
                word: word.to_string(),
                zipf,
            })
        })
        .collect()
}

/// Common words: those a word list (such as `/usr/share/dict/words`) has in
/// lowercase — so not names or brands — or whose base form it has once a
/// regular ending (`-s`, `-es`, `-ed`, `-ing`, `-er`, `-est`, `-ly`, `'s`)
/// is stripped.
pub struct CommonWords {
    words: std::collections::HashSet<String>,
}

impl CommonWords {
    /// Reads a word list, one word per line; capitalized entries (proper
    /// nouns) are ignored.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            words: text
                .lines()
                .map(str::trim)
                .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_lowercase()))
                .map(str::to_string)
                .collect(),
        })
    }

    pub fn contains(&self, word: &str) -> bool {
        if word.chars().count() == 1 {
            return matches!(word, "a" | "i" | "o");
        }
        if self.words.contains(word) {
            return true;
        }
        let base = word.trim_end_matches("'s");
        if base != word && self.words.contains(base) {
            return true;
        }
        for suffix in ["s", "es", "ed", "ing", "er", "est", "ly", "ies", "ied"] {
            if let Some(stem) = word.strip_suffix(suffix) {
                let mut stems = vec![stem.to_string(), format!("{stem}e"), format!("{stem}y")];
                // A doubled consonant: "running" → "run".
                let bytes = stem.as_bytes();
                if bytes.len() > 2 && bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
                    stems.push(stem[..stem.len() - 1].to_string());
                }
                if stems.iter().any(|s| s.len() > 1 && self.words.contains(s)) {
                    return true;
                }
            }
        }
        false
    }
}

impl Lexicon {
    /// The `max_words` most frequent words that CMUdict can pronounce, that
    /// are spelled with letters and apostrophes only, and that `keep`
    /// accepts — with every CMUdict pronunciation, or only the first.
    pub fn build(
        dict: &CmuDictionary,
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
            let first: Vec<Ph> = pronunciations[0].iter().map(Ph::from_phoneme).collect();
            let next = sounds.len() as u32;
            lexicon
                .sound
                .push(*sounds.entry(first.clone()).or_insert(next));
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

    /// `−ln P(word)`: about 3 for "the", 8 for "nice", 15 for "aardvark".
    pub fn surprisal(&self, id: WordId) -> f32 {
        self.surprisal[id as usize]
    }

    /// Reads word-pair counts (`left<TAB>right<TAB>count`, with
    /// `left<TAB>*<TAB>total` lines for the left words' totals), for
    /// [`Self::surprisal_after`], smoothed with strength `prior` (μ; see
    /// [`BIGRAM_PRIOR`]).
    pub fn load_bigrams(&mut self, path: &Path, prior: f32) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut totals = vec![0.0f32; self.len()];
        let mut pairs = HashMap::new();
        for line in text.lines() {
            let mut fields = line.split('\t');
            let (Some(left), Some(right), Some(count)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(format!("{}: malformed line {line:?}", path.display()));
            };
            let count: f32 = count
                .parse()
                .map_err(|e| format!("{}: {line:?}: {e}", path.display()))?;
            let Some(v) = self.id(left) else { continue };
            if right == "*" {
                totals[v as usize] = count;
            } else if let Some(w) = self.id(right) {
                pairs.insert((v, w), count);
            }
        }
        let mut best: Vec<f32> = self.surprisal.clone();
        for (&(v, w), &count) in &pairs {
            let p = (count + prior * (-self.surprisal[w as usize]).exp())
                / (totals[v as usize] + prior);
            best[w as usize] = best[w as usize].min(-p.ln());
        }
        self.bigrams = Some(Bigrams {
            prior,
            totals,
            pairs,
            best,
        });
        Ok(())
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
                let count = b.pairs.get(&(v, word)).copied().unwrap_or(0.0);
                let p = (count + b.prior * (-unigram).exp()) / (b.totals[v as usize] + b.prior);
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
    fn common_words_keep_inflections_but_not_names() {
        let common = CommonWords {
            words: ["boy", "run", "love", "city", "a"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        for word in [
            "boy", "boys", "running", "loved", "cities", "boy's", "a", "i",
        ] {
            assert!(common.contains(word), "{word}");
        }
        for word in ["boyd", "deloitte", "b", "loveds"] {
            assert!(!common.contains(word), "{word}");
        }
    }
}
