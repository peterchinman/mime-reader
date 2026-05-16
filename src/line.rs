use std::{cmp::min, collections::HashSet};

use crate::{
    DamerauLevenshtein, Dictionary, Stress, SyllableCountSpecification,
    phoneme::{Phoneme, RhymingPart, get_last_n_syllables, get_rhyming_part},
};

pub struct Pronunciation {
    pub phonemes: Box<[Phoneme]>,
    pub stress_pattern: Vec<Stress>,
}

pub enum WordData {
    Unknown,
    Known(Vec<Pronunciation>),
}

impl WordData {
    pub fn stress_patterns(&self) -> HashSet<&[Stress]> {
        match self {
            WordData::Known(pronunciations) => pronunciations
                .iter()
                .map(|p| p.stress_pattern.as_slice())
                .collect(),
            WordData::Unknown => HashSet::new(),
        }
    }
}

pub struct WordEntry {
    pub word: String,
    pub data: WordData,
}

impl WordEntry {
    fn new(word: &str, dict: &Dictionary) -> Self {
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
    pub fn new(s: &str, dict: &Dictionary) -> Self {
        let words = s
            .split_whitespace()
            .map(|w| WordEntry::new(w, dict))
            .collect::<Vec<_>>();
        Self { words }
    }
}

fn rhyming_parts_of_last_word<'a>(line: &'a Line) -> HashSet<RhymingPart<'a>> {
    line.words
        .last()
        .and_then(|word| match &word.data {
            WordData::Known(ps) => Some(ps),
            WordData::Unknown => None,
        })
        .map(|ps| ps.iter().map(|p| get_rhyming_part(&p.phonemes)).collect())
        .unwrap_or_default()
}

// Returns the lowest distance score after comaring all the possible rhyming parts. Note: currently this compares the shortest
fn compare_rhyming_parts(a: &Line, b: &Line, dl: &DamerauLevenshtein) -> Option<u32> {
    let parts_self = rhyming_parts_of_last_word(a);
    let parts_other = rhyming_parts_of_last_word(b);

    parts_self
        .iter()
        .flat_map(|a| {
            parts_other.iter().filter_map(move |b| {
                let shortest_count = min(a.syllable_count, b.syllable_count);
                let a_phonemes = get_last_n_syllables(a.phonemes, shortest_count)?;
                let b_phonemes = get_last_n_syllables(b.phonemes, shortest_count)?;
                Some(dl.distance(a_phonemes, b_phonemes))
            })
        })
        .min()
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
        assert_eq!(line.words[0].data.stress_patterns().len(), 1);
    }

    #[test]
    fn original_words_are_preserved() {
        let line = Line::new("Hello World", dict());
        assert_eq!(line.words[0].word, "Hello");
        assert_eq!(line.words[1].word, "World");
    }
}
