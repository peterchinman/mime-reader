//! A dense phoneme × phoneme score table.

use std::sync::Arc;

use super::{PHONEME_IDS, all_phonemes, phoneme_id};
use crate::phoneme::Phoneme;

/// Pair scores for every (phoneme, phoneme) combination, indexed by
/// [`phoneme_id`]. Built once per configuration, so a DP cell costs one
/// array lookup instead of a distance-engine call.
///
/// Scores are maximized: a rhyme table holds `−cost`, an echo table holds a
/// similarity. The table is shared behind an [`Arc`] so cloning a scorer that
/// owns one is cheap.
#[derive(Clone, PartialEq, Eq)]
pub struct ScoreTable {
    scores: Arc<[i64]>,
}

impl ScoreTable {
    /// Builds the table by calling `score` once per ordered phoneme pair.
    pub fn from_fn(mut score: impl FnMut(&Phoneme, &Phoneme) -> i64) -> Self {
        let phonemes = all_phonemes();
        let mut scores = Vec::with_capacity(PHONEME_IDS * PHONEME_IDS);
        for a in &phonemes {
            for b in &phonemes {
                scores.push(score(a, b));
            }
        }
        Self {
            scores: scores.into(),
        }
    }

    /// The score of aligning the phoneme with id `a` against the one with id
    /// `b`.
    #[inline]
    pub fn get(&self, a: u8, b: u8) -> i64 {
        self.scores[usize::from(a) * PHONEME_IDS + usize::from(b)]
    }

    /// The score of aligning `a` against `b`.
    pub fn score(&self, a: &Phoneme, b: &Phoneme) -> i64 {
        self.get(phoneme_id(a), phoneme_id(b))
    }

    /// The [`phoneme_id`] of every phoneme in `phonemes`, for indexing.
    pub fn ids(phonemes: &[Phoneme]) -> Vec<u8> {
        phonemes.iter().map(phoneme_id).collect()
    }
}

impl std::fmt::Debug for ScoreTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScoreTable")
            .field("len", &self.scores.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Phoneme {
        s.parse().unwrap()
    }

    #[test]
    fn from_fn_visits_every_ordered_pair_once() {
        let mut calls = 0;
        let table = ScoreTable::from_fn(|_, _| {
            calls += 1;
            calls
        });
        assert_eq!(calls as usize, PHONEME_IDS * PHONEME_IDS);
        // Row-major: the first row is phoneme 0 against everything.
        assert_eq!(table.get(0, 0), 1);
        assert_eq!(table.get(0, 1), 2);
        assert_eq!(table.get(1, 0), PHONEME_IDS as i64 + 1);
    }

    #[test]
    fn lookup_by_phoneme_matches_the_function() {
        let table = ScoreTable::from_fn(|a, b| {
            if a == b {
                10
            } else if matches!(a, Phoneme::Vowel(_)) == matches!(b, Phoneme::Vowel(_)) {
                -1
            } else {
                -100
            }
        });
        assert_eq!(table.score(&p("AE1"), &p("AE1")), 10);
        assert_eq!(table.score(&p("AE1"), &p("AE0")), -1);
        assert_eq!(table.score(&p("T"), &p("D")), -1);
        assert_eq!(table.score(&p("T"), &p("AE1")), -100);
    }

    #[test]
    fn ids_index_the_table() {
        let phonemes: Vec<Phoneme> = ["K", "AE1", "T"].iter().map(|s| p(s)).collect();
        let ids = ScoreTable::ids(&phonemes);
        assert_eq!(ids, phonemes.iter().map(phoneme_id).collect::<Vec<_>>());
    }

    #[test]
    fn clones_share_storage() {
        let table = ScoreTable::from_fn(|_, _| 0);
        let clone = table.clone();
        assert!(Arc::ptr_eq(&table.scores, &clone.scores));
    }
}
