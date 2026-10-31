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
}

pub struct Line {
    pub words: Vec<WordEntry>,
}

impl Line {
    pub fn new<D: PronouncingDictionary + ?Sized>(s: &str, dict: &D) -> Self {
        let words = s
            .split_whitespace()
            .map(|w| WordEntry::new(w, dict))
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
}
