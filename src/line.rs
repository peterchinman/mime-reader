use std::collections::HashSet;

use crate::{PronouncingDictionary, Stress, phoneme::Phoneme};

pub struct Pronunciation {
    pub phonemes: Box<[Phoneme]>,
    pub stress_pattern: Vec<Stress>,
}

pub enum WordData {
    Unknown,
    Known(Vec<Pronunciation>),
}

impl WordData {
    /// Returns the stress patterns of the word, if known.
    pub fn stress_patterns(&self) -> Option<HashSet<&[Stress]>> {
        match self {
            WordData::Known(pronunciations) => Some(
                pronunciations
                    .iter()
                    .map(|p| p.stress_pattern.as_slice())
                    .collect(),
            ),
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
        let data = match dict.lookup(word) {
            None => WordData::Unknown,
            Some(raw) => {
                let pronunciations = raw
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
                WordData::Known(pronunciations)
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
                WordData::Known(_) => None,
                WordData::Unknown => Some(word.word.as_str()),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    struct TestDictionary;

    impl PronouncingDictionary for TestDictionary {
        fn lookup(&self, word: &str) -> Option<Vec<Box<[Phoneme]>>> {
            if word != "gato" {
                return None;
            }

            Some(vec![
                ["G", "AA1", "T", "OW0"]
                    .into_iter()
                    .map(|phoneme| phoneme.parse().unwrap())
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ])
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
    }

    #[test]
    fn accepts_custom_pronouncing_dictionary() {
        let line = Line::new("gato missing", &TestDictionary);

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
    fn word_with_multiple_pronunciations() {
        // "hello" has two entries in CMUdict: HELLO and HELLO(1)
        let line = Line::new("hello", dict());
        let WordData::Known(ref pronunciations) = line.words[0].data else {
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
}
