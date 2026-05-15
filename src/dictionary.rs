use std::collections::HashMap;
use std::path::Path;

use crate::error::DictionaryError;
use crate::phoneme::Phoneme;

pub struct Dictionary {
    entries: HashMap<String, Vec<Box<[Phoneme]>>>,
}

impl Dictionary {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        let bytes = std::fs::read(path).map_err(DictionaryError::Io)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DictionaryError> {
        let content = bytes.iter().map(|&b| b as char).collect::<String>();

        let mut entries: HashMap<String, Vec<Box<[Phoneme]>>> = HashMap::new();

        for line in content.lines() {
            if line.starts_with(";;;") {
                continue;
            }

            let (word, phonemes_str) = line
                .split_once("  ")
                .ok_or_else(|| DictionaryError::MalformedLine(line.to_string()))?;

            // Strip alternate pronunciation suffix: HELLO(1) -> HELLO
            let word = match word.find('(') {
                Some(idx) => &word[..idx],
                None => word,
            };

            let phonemes: Vec<Phoneme> = phonemes_str
                .split_whitespace()
                .map(|s| s.parse::<Phoneme>())
                .collect::<Result<_, _>>()
                .map_err(DictionaryError::InvalidPhoneme)?;

            entries
                .entry(word.to_lowercase())
                .or_default()
                .push(phonemes.into_boxed_slice());
        }

        Ok(Dictionary { entries })
    }

    pub fn lookup(&self, word: &str) -> Option<Vec<Box<[Phoneme]>>> {
        self.entries.get(&word.to_lowercase()).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phoneme::{Phoneme, Stress};
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<Dictionary> = OnceLock::new();

    fn dict() -> &'static Dictionary {
        DICT.get_or_init(|| Dictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    #[test]
    fn loads_without_error() {
        dict();
    }

    #[test]
    fn known_word_returns_pronunciations() {
        let dict = dict();
        assert!(!dict.lookup("hello").unwrap().is_empty());
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let dict = dict();
        assert_eq!(
            dict.lookup("hello").unwrap().len(),
            dict.lookup("HELLO").unwrap().len()
        );
    }

    #[test]
    fn word_with_alternate_pronunciations() {
        let dict = dict();
        // HELLO has two entries in CMUdict: HELLO and HELLO(1)
        assert_eq!(dict.lookup("hello").unwrap().len(), 2);
    }

    #[test]
    fn unknown_word_returns_none() {
        let dict = dict();
        assert!(dict.lookup("xyzzy").is_none());
    }

    #[test]
    fn pronunciation_phonemes_parse_correctly() {
        let dict = dict();
        // THE  DH AH0 — a simple known entry
        let pronunciations = dict.lookup("the").unwrap();
        let phonemes = &pronunciations[0];
        assert_eq!(phonemes.len(), 2);
        assert!(matches!(phonemes[0], Phoneme::Consonant(_)));
        assert!(matches!(phonemes[1], Phoneme::Vowel(_)));
    }

    #[test]
    fn vowel_stress_is_parsed() {
        let dict = dict();
        // HELLO  HH AH0 L OW1 — AH0 is unstressed, OW1 is primary
        let pronunciations = dict.lookup("hello").unwrap();
        let phonemes = &pronunciations[0];
        let stresses: Vec<&Stress> = phonemes
            .iter()
            .filter_map(|p| match p {
                Phoneme::Vowel(v) => Some(&v.stress),
                Phoneme::Consonant(_) => None,
            })
            .collect();
        assert!(matches!(stresses[0], Stress::Unstressed));
        assert!(matches!(stresses[1], Stress::Primary));
    }
}
