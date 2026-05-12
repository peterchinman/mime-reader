use std::collections::HashMap;
use std::path::Path;

use crate::error::DictionaryError;
use crate::phone::Phone;

pub struct Dictionary {
    entries: HashMap<String, Vec<Box<[Phone]>>>,
}

impl Dictionary {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        let bytes = std::fs::read(path).map_err(DictionaryError::Io)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DictionaryError> {
        let content = bytes.iter().map(|&b| b as char).collect::<String>();

        let mut entries: HashMap<String, Vec<Box<[Phone]>>> = HashMap::new();

        for line in content.lines() {
            if line.starts_with(";;;") {
                continue;
            }

            let (word, phones_str) = line
                .split_once("  ")
                .ok_or_else(|| DictionaryError::MalformedLine(line.to_string()))?;

            // Strip alternate pronunciation suffix: HELLO(1) -> HELLO
            let word = match word.find('(') {
                Some(idx) => &word[..idx],
                None => word,
            };

            let phones: Vec<Phone> = phones_str
                .split_whitespace()
                .map(|s| s.parse::<Phone>())
                .collect::<Result<_, _>>()
                .map_err(DictionaryError::InvalidPhone)?;

            entries
                .entry(word.to_lowercase())
                .or_default()
                .push(phones.into_boxed_slice());
        }

        Ok(Dictionary { entries })
    }

    // Returns an array of pronunciations for a given word, if known.
    pub fn word_to_phones(&self, word: &str) -> Result<&Vec<Box<[Phone]>>, DictionaryError> {
        self.entries
            .get(&word.to_lowercase())
            .ok_or_else(|| DictionaryError::UnknownWord(word.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::{Phone, Stress};
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
        let pronunciations = dict.word_to_phones("hello").unwrap();
        assert!(!pronunciations.is_empty());
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let dict = dict();
        let lower = dict.word_to_phones("hello").unwrap();
        let upper = dict.word_to_phones("HELLO").unwrap();
        assert_eq!(lower.len(), upper.len());
    }

    #[test]
    fn word_with_alternate_pronunciations() {
        let dict = dict();
        // HELLO has two entries in CMUdict: HELLO and HELLO(1)
        let pronunciations = dict.word_to_phones("hello").unwrap();
        assert_eq!(pronunciations.len(), 2);
    }

    #[test]
    fn unknown_word_returns_error() {
        let dict = dict();
        let err = dict.word_to_phones("xyzzy").unwrap_err();
        assert!(matches!(err, DictionaryError::UnknownWord(_)));
    }

    #[test]
    fn pronunciation_phones_parse_correctly() {
        let dict = dict();
        // THE  DH AH0 — a simple known entry
        let pronunciations = dict.word_to_phones("the").unwrap();
        let phones = &pronunciations[0];
        assert_eq!(phones.len(), 2);
        assert!(matches!(phones[0], Phone::Consonant(_)));
        assert!(matches!(phones[1], Phone::Vowel(_)));
    }

    #[test]
    fn vowel_stress_is_parsed() {
        let dict = dict();
        // HELLO  HH AH0 L OW1 — AH0 is unstressed, OW1 is primary
        let phones = &dict.word_to_phones("hello").unwrap()[0];
        let stresses: Vec<&Stress> = phones.iter().filter_map(|p| match p {
            Phone::Vowel(v) => Some(&v.stress),
            Phone::Consonant(_) => None,
        }).collect();
        assert!(matches!(stresses[0], Stress::Unstressed));
        assert!(matches!(stresses[1], Stress::Primary));
    }
}
