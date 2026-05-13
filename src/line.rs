use std::collections::HashSet;

use crate::{Dictionary, Stress, phoneme::Phoneme};

pub enum WordData {
    Unknown,
    Known {
        pronunciations: Vec<Box<[Phoneme]>>,
        stress_patterns: HashSet<Vec<Stress>>,
    },
}

pub struct WordEntry {
    pub word: String,
    pub data: WordData,
}

impl WordEntry {
    fn new(word: &str, dict: &Dictionary) -> Self {
        let data = match dict.lookup(word) {
            None => WordData::Unknown,
            Some(pronunciations) => {
                let stress_patterns = pronunciations
                    .iter()
                    .map(|pronunciation| {
                        pronunciation
                            .iter()
                            .filter_map(|phoneme| match phoneme {
                                Phoneme::Vowel(vowel) => Some(vowel.stress),
                                Phoneme::Consonant(_) => None,
                            })
                            .collect()
                    })
                    .collect();
                WordData::Known { pronunciations, stress_patterns }
            }
        };
        Self { word: word.to_string(), data }
    }
}

pub struct Line {
    pub words: Vec<WordEntry>,
}

impl Line {
    pub fn new(s: &str, dict: &Dictionary) -> Self {
        let words = s.split_whitespace()
            .map(|w| WordEntry::new(w, dict))
            .collect::<Vec<_>>();
        Self { words }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<Dictionary> = OnceLock::new();

    fn dict() -> &'static Dictionary {
        DICT.get_or_init(|| Dictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    #[test]
    fn known_words_have_pronunciations() {
        let line = Line::new("hello world", dict());
        assert_eq!(line.words.len(), 2);
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Known { .. }));
    }

    #[test]
    fn unknown_word_has_no_pronunciations() {
        let line = Line::new("hello xyzzy", dict());
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Unknown));
    }

    #[test]
    fn word_with_multiple_pronunciations() {
        // "hello" has two entries in CMUdict: HELLO and HELLO(1)
        let line = Line::new("hello", dict());
        let WordData::Known { ref pronunciations, .. } = line.words[0].data else { panic!() };
        assert_eq!(pronunciations.len(), 2);
    }

    #[test]
    fn original_words_are_preserved() {
        let line = Line::new("Hello World", dict());
        assert_eq!(line.words[0].word, "Hello");
        assert_eq!(line.words[1].word, "World");
    }

    #[test]
    fn stress_patterns_precomputed_for_known_word() {
        let line = Line::new("hello", dict());
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
    }

    #[test]
    fn stress_patterns_none_for_unknown_word() {
        let line = Line::new("xyzzy", dict());
        assert!(matches!(line.words[0].data, WordData::Unknown));
    }
}
