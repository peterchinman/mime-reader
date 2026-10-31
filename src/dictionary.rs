use std::collections::HashMap;
use std::path::Path;

use crate::error::CmuDictionaryError;
use crate::phoneme::Phoneme;

pub type Pronunciations = Vec<Box<[Phoneme]>>;

pub trait PronouncingDictionary {
    /// The pronunciations of `word`, borrowed from the dictionary — callers
    /// that need ownership clone explicitly.
    fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]>;
    fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])>;
}

impl<T: PronouncingDictionary + ?Sized> PronouncingDictionary for &T {
    fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
        (**self).lookup(word)
    }
    fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
        (**self).entries()
    }
}

impl<T: PronouncingDictionary + ?Sized> PronouncingDictionary for Box<T> {
    fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
        (**self).lookup(word)
    }
    fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
        (**self).entries()
    }
}

pub struct CmuDictionary {
    entries: HashMap<String, Pronunciations>,
}

impl CmuDictionary {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CmuDictionaryError> {
        let bytes = std::fs::read(path).map_err(CmuDictionaryError::Io)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CmuDictionaryError> {
        let content = bytes.iter().map(|&b| b as char).collect::<String>();

        let mut entries: HashMap<String, Pronunciations> = HashMap::new();

        for line in content.lines() {
            if line.starts_with(";;;") {
                continue;
            }

            let (word, phonemes_str) = line
                .split_once("  ")
                .ok_or_else(|| CmuDictionaryError::MalformedLine(line.to_string()))?;

            // Strip alternate pronunciation suffix: HELLO(1) -> HELLO. Only a
            // trailing number counts, since headwords such as (PAREN begin
            // with a parenthesis.
            let word = word
                .strip_suffix(')')
                .and_then(|word| word.rsplit_once('('))
                .filter(|(base, number)| {
                    !base.is_empty()
                        && !number.is_empty()
                        && number.bytes().all(|byte| byte.is_ascii_digit())
                })
                .map_or(word, |(base, _)| base);

            let phonemes: Vec<Phoneme> = phonemes_str
                .split_whitespace()
                .map(|s| s.parse::<Phoneme>())
                .collect::<Result<_, _>>()
                .map_err(CmuDictionaryError::InvalidPhoneme)?;

            entries
                .entry(word.to_lowercase())
                .or_default()
                .push(phonemes.into_boxed_slice());
        }

        Ok(CmuDictionary { entries })
    }

    pub fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
        self.entries.get(&word.to_lowercase()).map(Vec::as_slice)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
        self.entries
            .iter()
            .map(|(word, pronunciations)| (word.as_str(), pronunciations.as_slice()))
    }
}

impl PronouncingDictionary for CmuDictionary {
    fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
        CmuDictionary::lookup(self, word)
    }
    fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
        CmuDictionary::iter(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phoneme::{Phoneme, Stress};
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
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
    fn pronunciations_keep_dictionary_order() {
        // Callers read [0] as a word's primary pronunciation: cmudict lists
        // the base entry before its (1)/(2) variants, and from_bytes pushes
        // pronunciations in line order. LIVE's base entry is "L AY1 V";
        // "L IH1 V" is the LIVE(1) variant.
        let dict = dict();
        let live: Vec<String> = dict.lookup("live").unwrap()[0]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(live, ["L", "AY1", "V"]);
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

    #[test]
    fn headwords_that_begin_with_a_parenthesis_keep_it() {
        let dict = dict();
        assert!(dict.lookup("").is_none());
        assert!(dict.iter().all(|(word, _)| !word.is_empty()));
        assert_eq!(dict.lookup("(paren").unwrap().len(), 1);
        assert!(dict.lookup(")close-paren").is_some());

        let sparse =
            CmuDictionary::from_bytes(b"(PAREN  P ER0 EH1 N\n(PAREN(1)  P AE1 R AH0 N\n").unwrap();
        assert_eq!(sparse.lookup("(paren").unwrap().len(), 2);
    }
}
