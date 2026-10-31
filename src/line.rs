use std::collections::HashSet;

use crate::{PronouncingDictionary, PronunciationOrigin, Stress, phoneme::Phoneme};

pub struct Pronunciation {
    pub phonemes: Box<[Phoneme]>,
    pub stress_pattern: Vec<Stress>,
}

pub enum WordData {
    Unknown,
    Known {
        pronunciations: Vec<Pronunciation>,
        origin: PronunciationOrigin,
    },
}

impl WordData {
    /// Returns the stress patterns of the word, if known.
    pub fn stress_patterns(&self) -> Option<HashSet<&[Stress]>> {
        match self {
            WordData::Known { pronunciations, .. } => Some(
                pronunciations
                    .iter()
                    .map(|p| p.stress_pattern.as_slice())
                    .collect(),
            ),
            WordData::Unknown => None,
        }
    }

    /// Where this word's pronunciations came from, or `None` when it is unknown.
    pub fn origin(&self) -> Option<&PronunciationOrigin> {
        match self {
            WordData::Known { origin, .. } => Some(origin),
            WordData::Unknown => None,
        }
    }
}

pub struct WordEntry {
    pub word: String,
    pub data: WordData,
}

impl WordEntry {
    fn new<D: PronouncingDictionary + ?Sized>(word: &str, dict: &D) -> Self {
        let data = match dict.pronunciations_for(word) {
            None => WordData::Unknown,
            Some(word_pronunciations) => {
                let pronunciations = word_pronunciations
                    .pronunciations
                    .into_owned()
                    .into_iter()
                    .map(|phonemes| {
                        let stress_pattern = phonemes
                            .iter()
                            .filter_map(|phoneme| match phoneme {
                                Phoneme::Vowel(vowel) => Some(vowel.stress),
                                Phoneme::Consonant(_) => None,
                            })
                            .collect();
                        Pronunciation {
                            phonemes,
                            stress_pattern,
                        }
                    })
                    .collect();
                WordData::Known {
                    pronunciations,
                    origin: word_pronunciations.origin,
                }
            }
        };
        Self {
            word: word.to_string(),
            data,
        }
    }

    /// Resolves one whitespace-separated token of running text, ignoring the
    /// punctuation around a word unless the dictionary spells the word with
    /// it. Tries the token as written (*Jr.*, *'tis*), then without
    /// surrounding punctuation (*light.* -> *light*), then also without
    /// surrounding apostrophes, which may be quotation marks (*'hello'* ->
    /// *hello*). An unknown word keeps its apostrophes (*xyzzin'*). Returns
    /// `None` for a token with no letters or digits, such as a dash.
    fn from_token<D: PronouncingDictionary + ?Sized>(token: &str, dict: &D) -> Option<Self> {
        let unpunctuated = token.trim_matches(|character: char| {
            !character.is_alphanumeric() && !is_apostrophe(character)
        });
        let unquoted = unpunctuated.trim_matches(is_apostrophe);

        // Each spelling contains the next, so any repeats are adjacent.
        let mut spellings = vec![token, unpunctuated, unquoted];
        spellings.dedup();

        spellings
            .into_iter()
            .filter(|spelling| !spelling.is_empty())
            .map(|spelling| Self::new(spelling, dict))
            .find(|entry| matches!(entry.data, WordData::Known { .. }))
            .or_else(|| {
                token.chars().any(char::is_alphanumeric).then(|| Self {
                    word: unpunctuated.to_string(),
                    data: WordData::Unknown,
                })
            })
    }
}

/// Straight and typographic apostrophes, which can belong to a word ('tis,
/// kettles') where other punctuation cannot.
fn is_apostrophe(character: char) -> bool {
    matches!(character, '\'' | '’')
}

pub struct Line {
    pub words: Vec<WordEntry>,
}

impl Line {
    /// Splits `s` into words at whitespace. Punctuation around a word is
    /// ignored unless the dictionary spells the word with it, and a token
    /// with no letters or digits, such as a dash, is not a word. Word indices
    /// therefore count words, not whitespace-separated tokens.
    pub fn new<D: PronouncingDictionary + ?Sized>(s: &str, dict: &D) -> Self {
        let words = s
            .split_whitespace()
            .filter_map(|token| WordEntry::from_token(token, dict))
            .collect::<Vec<_>>();
        Self { words }
    }
    pub fn unknown_words(&self) -> HashSet<&str> {
        self.words
            .iter()
            .filter_map(|word| match &word.data {
                WordData::Known { .. } => None,
                WordData::Unknown => Some(word.word.as_str()),
            })
            .collect()
    }

    /// The distinct total syllable counts across the line's pronunciation
    /// combinations, first-seen order — a one-word line reproduces the
    /// word's per-pronunciation counts. `None` if any word is unknown or
    /// the line is empty.
    pub fn syllable_counts(&self) -> Option<Vec<usize>> {
        if self.words.is_empty() {
            return None;
        }
        let mut totals: Vec<usize> = vec![0];
        for entry in &self.words {
            let WordData::Known { pronunciations, .. } = &entry.data else {
                return None;
            };
            let mut next: Vec<usize> = Vec::new();
            for total in &totals {
                for pronunciation in pronunciations {
                    let sum = total + pronunciation.stress_pattern.len();
                    if !next.contains(&sum) {
                        next.push(sum);
                    }
                }
            }
            totals = next;
        }
        Some(totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use crate::dictionary::Pronunciations;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    // Owns its one entry: the trait lends pronunciations out, so a
    // dictionary can't fabricate them per lookup.
    struct TestDictionary {
        gato: Pronunciations,
    }

    impl TestDictionary {
        fn new() -> Self {
            TestDictionary {
                gato: vec![
                    ["G", "AA1", "T", "OW0"]
                        .into_iter()
                        .map(|phoneme| phoneme.parse().unwrap())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ],
            }
        }
    }

    impl PronouncingDictionary for TestDictionary {
        fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
            (word == "gato").then_some(self.gato.as_slice())
        }
        fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
            std::iter::empty()
        }
    }

    // --- Line ---

    #[test]
    fn blank_line_has_no_words() {
        let line = Line::new("", dict());
        assert!(line.words.is_empty());
    }

    #[test]
    fn known_words_have_pronunciations() {
        let line = Line::new("hello world", dict());
        assert_eq!(line.words.len(), 2);
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Known { .. }));
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Dictionary { normalized: false })
        );
    }

    #[test]
    fn accepts_custom_pronouncing_dictionary() {
        let line = Line::new("gato missing", &TestDictionary::new());

        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Unknown));
    }

    #[test]
    fn unknown_word_returns_unknown() {
        let line = Line::new("hello xyzzy", dict());
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Unknown));
    }

    #[test]
    fn derived_words_require_opt_in_and_keep_their_base_syllable_count() {
        let strict = Line::new("kettle’s", dict());
        assert_eq!(strict.unknown_words(), HashSet::from(["kettle’s"]));
        assert_eq!(strict.syllable_counts(), None);

        let permissive = dict().with_pronunciation_mode(crate::PronunciationMode::AllowDerived);
        let line = Line::new("kettle’s", &permissive);

        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Derived {
                dictionary_base: "kettle".to_string(),
                rules: vec![crate::InflectionRule::ApostropheS],
            })
        );
        assert_eq!(line.syllable_counts(), Some(vec![2]));
    }

    #[test]
    fn word_with_multiple_pronunciations() {
        // "hello" has two entries in CMUdict: HELLO and HELLO(1)
        let line = Line::new("hello", dict());
        let WordData::Known {
            ref pronunciations, ..
        } = line.words[0].data
        else {
            panic!()
        };
        assert_eq!(pronunciations.len(), 2);
    }

    #[test]
    fn stress_patterns_deduplicated() {
        // both pronunciations of "hello" have the same stress pattern, so HashSet should have 1 entry
        let line = Line::new("hello", dict());
        assert_eq!(line.words[0].data.stress_patterns().unwrap().len(), 1);
    }

    #[test]
    fn original_words_are_preserved() {
        let line = Line::new("Hello World", dict());
        assert_eq!(line.words[0].word, "Hello");
        assert_eq!(line.words[1].word, "World");
    }

    #[test]
    fn syllable_counts_sum_across_words() {
        // ice (1) + cream (1) = 2.
        assert_eq!(
            Line::new("ice cream", dict()).syllable_counts(),
            Some(vec![2])
        );
        // Words with multiple pronunciations sum per combination: fire (2 or
        // 1) + truck (1) gives distinct totals in first-seen order.
        assert_eq!(
            Line::new("fire truck", dict()).syllable_counts(),
            Some(vec![3, 2])
        );
        // Unknown words and empty lines have no counts.
        assert_eq!(Line::new("qzxqzx", dict()).syllable_counts(), None);
        assert_eq!(Line::new("", dict()).syllable_counts(), None);
    }

    // --- Punctuation ---

    fn words(line: &Line) -> Vec<&str> {
        line.words.iter().map(|word| word.word.as_str()).collect()
    }

    #[test]
    fn surrounding_punctuation_is_ignored() {
        let line = Line::new("“Hello,” she said; (light.)", dict());
        assert_eq!(words(&line), ["Hello", "she", "said", "light"]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn tokens_without_letters_or_digits_are_not_words() {
        let line = Line::new("cat — dog … & ' ", dict());
        assert_eq!(words(&line), ["cat", "dog"]);
        assert_eq!(line.syllable_counts(), Some(vec![2]));
        assert!(Line::new("— …", dict()).words.is_empty());
    }

    #[test]
    fn dictionary_spellings_keep_their_punctuation() {
        let line = Line::new("Jr. U.S. 'tis, dancin'!", dict());
        assert_eq!(words(&line), ["Jr.", "U.S.", "'tis", "dancin'"]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn edge_apostrophes_are_dropped_only_when_the_word_needs_it() {
        let line = Line::new("'hello' ‘world’ xyzzin',", dict());
        assert_eq!(words(&line), ["hello", "world", "xyzzin'"]);
        assert_eq!(line.unknown_words(), HashSet::from(["xyzzin'"]));
    }

    #[test]
    fn plural_possessives_keep_their_apostrophe_when_derivable() {
        // Without derivation, a plural possessive reads as its plural.
        let strict = Line::new("kettles’,", dict());
        assert_eq!(words(&strict), ["kettles"]);

        let permissive = dict().with_pronunciation_mode(crate::PronunciationMode::AllowDerived);
        let line = Line::new("kettles’,", &permissive);
        assert_eq!(words(&line), ["kettles’"]);
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Derived {
                dictionary_base: "kettles".to_string(),
                rules: vec![crate::InflectionRule::SApostrophe],
            })
        );
    }
}
