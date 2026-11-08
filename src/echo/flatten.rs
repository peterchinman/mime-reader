//! Lines as single phoneme sequences.
//!
//! Echo is boundary-blind: a line is aligned as one run of phonemes, so
//! "demands I" can echo "demand sigh". Word starts are kept only to map
//! results back to the text.

use std::ops::Range;

use crate::error::{EchoError, UnknownWordError};
use crate::line::{Line, WordData};
use crate::phoneme::Phoneme;

/// A line flattened to one phoneme sequence, under one choice of
/// pronunciation per word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatLine {
    pub phonemes: Vec<Phoneme>,
    /// `word_starts[k]`: the index in `phonemes` where word `k` begins.
    pub word_starts: Vec<usize>,
    /// `pronunciation_choice[k]`: which of word `k`'s pronunciations was used.
    pub pronunciation_choice: Vec<usize>,
}

impl FlatLine {
    /// Concatenates `words`' pronunciations (each word's first choice).
    pub fn from_words(words: &[&[Phoneme]]) -> Self {
        let mut phonemes = Vec::new();
        let mut word_starts = Vec::with_capacity(words.len());
        for word in words {
            word_starts.push(phonemes.len());
            phonemes.extend_from_slice(word);
        }
        Self {
            phonemes,
            word_starts,
            pronunciation_choice: vec![0; words.len()],
        }
    }

    pub fn word_count(&self) -> usize {
        self.word_starts.len()
    }

    /// The word containing phoneme `index`.
    pub fn word_at(&self, index: usize) -> usize {
        self.word_starts
            .partition_point(|&start| start <= index)
            .saturating_sub(1)
    }

    /// The phonemes of word `word`.
    pub fn word_range(&self, word: usize) -> Range<usize> {
        let end = self
            .word_starts
            .get(word + 1)
            .copied()
            .unwrap_or(self.phonemes.len());
        self.word_starts[word]..end
    }
}

/// Every word's pronunciations, or the first unknown word as an error.
fn pronunciations(line: &Line) -> Result<Vec<Vec<&[Phoneme]>>, EchoError> {
    if line.words.is_empty() {
        return Err(EchoError::EmptyLine);
    }
    line.words
        .iter()
        .map(|entry| match &entry.data {
            WordData::Known { pronunciations, .. } => Ok(pronunciations
                .iter()
                .map(|pronunciation| &*pronunciation.phonemes)
                .collect()),
            WordData::Unknown => Err(EchoError::UnknownWord(UnknownWordError {
                word: entry.word.clone(),
            })),
        })
        .collect()
}

/// `line` with each word's first (primary) pronunciation.
#[cfg(test)]
fn flatten(line: &Line) -> Result<FlatLine, EchoError> {
    Ok(first_pronunciations(&pronunciations(line)?))
}

fn first_pronunciations(words: &[Vec<&[Phoneme]>]) -> FlatLine {
    let first: Vec<&[Phoneme]> = words.iter().map(|choices| choices[0]).collect();
    FlatLine::from_words(&first)
}

/// Every combination of the words' pronunciations, word 0 varying fastest.
/// When there would be more than `cap`, just the first-pronunciation
/// flattening.
pub(crate) fn flatten_variants(line: &Line, cap: usize) -> Result<Vec<FlatLine>, EchoError> {
    let words = pronunciations(line)?;
    let total = words
        .iter()
        .try_fold(1usize, |total, choices| total.checked_mul(choices.len()));
    let Some(total) = total.filter(|&total| total <= cap.max(1)) else {
        return Ok(vec![first_pronunciations(&words)]);
    };
    let mut variants = Vec::with_capacity(total);
    for mut index in 0..total {
        let mut choice = Vec::with_capacity(words.len());
        for choices in &words {
            choice.push(index % choices.len());
            index /= choices.len();
        }
        let selected: Vec<&[Phoneme]> = words
            .iter()
            .zip(&choice)
            .map(|(choices, &k)| choices[k])
            .collect();
        let mut flat = FlatLine::from_words(&selected);
        flat.pronunciation_choice = choice;
        variants.push(flat);
    }
    Ok(variants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{PronouncingDictionary, Pronunciations};
    use crate::echo::test_support::{dict, parse};

    #[test]
    fn word_starts_map_phonemes_back_to_words() {
        let (dark, demands) = (parse("D AA1 R K"), parse("D IH0 M AE1 N D Z"));
        let flat = FlatLine::from_words(&[&dark, &demands]);
        assert_eq!(flat.word_starts, [0, 4]);
        assert_eq!(flat.phonemes.len(), 11);
        assert_eq!(flat.word_at(0), 0);
        assert_eq!(flat.word_at(3), 0);
        assert_eq!(flat.word_at(4), 1);
        assert_eq!(flat.word_at(10), 1);
        assert_eq!(flat.word_range(1), 4..11);
    }

    #[test]
    fn flatten_uses_each_words_first_pronunciation() {
        let line = Line::new("call my own", dict());
        let flat = flatten(&line).unwrap();
        assert_eq!(flat.phonemes, parse("K AO1 L M AY1 OW1 N"));
        assert_eq!(flat.word_starts, [0, 3, 5]);
        assert_eq!(flat.pronunciation_choice, [0, 0, 0]);
    }

    #[test]
    fn variants_enumerate_pronunciation_combinations() {
        // FIRE has two pronunciations in CMUdict; TRUCK one.
        let line = Line::new("fire truck", dict());
        let variants = flatten_variants(&line, 64).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].pronunciation_choice, [0, 0]);
        assert_eq!(variants[1].pronunciation_choice, [1, 0]);
        assert_ne!(variants[0].phonemes, variants[1].phonemes);
    }

    struct FivefoldDictionary {
        pronunciations: Pronunciations,
    }

    impl PronouncingDictionary for FivefoldDictionary {
        fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
            ["ka", "ke", "ki", "ko"]
                .contains(&word)
                .then_some(self.pronunciations.as_slice())
        }
        fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
            std::iter::empty()
        }
    }

    #[test]
    fn too_many_variants_fall_back_to_first_pronunciations() {
        let dict = FivefoldDictionary {
            pronunciations: ["K AA1", "K AE1", "K IY1", "K OW1", "K UW1"]
                .iter()
                .map(|p| parse(p).into_boxed_slice())
                .collect(),
        };
        // 5^4 = 625 combinations > 64.
        let line = Line::new("ka ke ki ko", &dict);
        let variants = flatten_variants(&line, 64).unwrap();
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0], flatten(&line).unwrap());
        assert_eq!(flatten_variants(&line, 625).unwrap().len(), 625);
    }

    #[test]
    fn unknown_and_empty_lines_are_errors() {
        assert_eq!(
            flatten(&Line::new("call my xyzzy", dict())),
            Err(EchoError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string()
            }))
        );
        assert_eq!(
            flatten(&Line::new("  — ", dict())),
            Err(EchoError::EmptyLine)
        );
        assert_eq!(
            flatten_variants(&Line::new("", dict()), 8),
            Err(EchoError::EmptyLine)
        );
    }
}
