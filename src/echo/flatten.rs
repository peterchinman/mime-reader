//! Lines as single phoneme sequences.
//!
//! Echo is boundary-blind: a line is aligned as one run of phonemes, so
//! "demands I" can echo "demand sigh". Word starts are kept only to map
//! results back to the text.
//!
//! Each word takes its **first pronunciation** (CMUdict's primary one).
//! Echo deliberately doesn't search over pronunciation variants. Taking the
//! best of every combination gives unrelated lines more chances to match
//! by accident: on the labeled corpus (`data/echo/README.md`) variants
//! raised 87% of non-echo scores (by +0.025 on average) but only 41% of
//! echo scores (+0.023) — most echoes repeat the same words, which already
//! share a first pronunciation. So they bought no accuracy (semi-global was
//! slightly worse with them on every fold; Waterman–Eggert unchanged),
//! cost 45–65× the time, and needed a cap that scored long lines by
//! different rules from short ones. An echo that only matches under an
//! alternative pronunciation ("live" as L AY1 V, against "alive") scores
//! lower; if real poems turn out to need that, it calls for a targeted
//! choice of pronunciation, not a search over all of them.

use std::ops::Range;

use crate::error::{EchoError, UnknownWordError};
use crate::line::{Line, WordData};
use crate::phoneme::Phoneme;

/// A line flattened to one phoneme sequence, each word under its first
/// pronunciation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatLine {
    pub phonemes: Vec<Phoneme>,
    /// `word_starts[k]`: the index in `phonemes` where word `k` begins.
    pub word_starts: Vec<usize>,
}

impl FlatLine {
    /// Concatenates `words`' phonemes.
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

/// `line` with each word's first pronunciation (see the module docs), or
/// the first unknown word as an error.
pub(crate) fn flatten(line: &Line) -> Result<FlatLine, EchoError> {
    if line.words.is_empty() {
        return Err(EchoError::EmptyLine);
    }
    let words = line
        .words
        .iter()
        .map(|entry| match &entry.data {
            WordData::Known { pronunciations, .. } => Ok(&*pronunciations[0].phonemes),
            WordData::Unknown => Err(EchoError::UnknownWord(UnknownWordError {
                word: entry.word.clone(),
            })),
        })
        .collect::<Result<Vec<&[Phoneme]>, EchoError>>()?;
    Ok(FlatLine::from_words(&words))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn only_first_pronunciations_are_used() {
        // FIRE has two pronunciations in CMUdict.
        let fire = Line::new("fire", dict());
        let WordData::Known { pronunciations, .. } = &fire.words[0].data else {
            panic!("fire is in CMUdict");
        };
        assert!(pronunciations.len() > 1);
        assert_eq!(
            flatten(&fire).unwrap().phonemes,
            &*pronunciations[0].phonemes
        );
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
        assert_eq!(flatten(&Line::new("", dict())), Err(EchoError::EmptyLine));
    }
}
