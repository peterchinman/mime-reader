use std::collections::HashSet;

use crate::{
    DamerauLevenshtein, Dictionary, Stress,
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

/// Returns all possible phoneme sequences representing the last `n` syllables of the line,
/// considering all pronunciation combinations across word boundaries.
fn last_n_syllables_of_line(words: &[WordEntry], n: usize) -> HashSet<Vec<Phoneme>> {
    if n == 0 {
        return HashSet::from([vec![]]);
    }

    let Some(last_word) = words.last() else {
        return HashSet::new();
    };

    let WordData::Known(pronunciations) = &last_word.data else {
        return HashSet::new();
    };

    let preceding_words = &words[..words.len() - 1];
    let mut results = HashSet::new();

    for pronunciation in pronunciations {
        let syl_count = pronunciation.stress_pattern.len();

        if syl_count >= n {
            if let Some(slice) = get_last_n_syllables(&pronunciation.phonemes, n) {
                results.insert(slice.to_vec());
            }
        } else {
            let needed = n - syl_count;
            for mut preceding in last_n_syllables_of_line(preceding_words, needed) {
                preceding.extend_from_slice(&pronunciation.phonemes);
                results.insert(preceding);
            }
        }
    }

    results
}

/// Compares (all of) the (possible) rhyming part(s) of the last word of Line A against the same sized portion of Line B.
/// Returns the minimal distance between the two sections divided by the syllable count of the rhyming part.
pub fn compare_rhyming_parts(a: &Line, b: &Line, dl: &DamerauLevenshtein) -> Option<f32> {
    let parts_a = rhyming_parts_of_last_word(a);

    parts_a
        .iter()
        .filter(|rp_a| rp_a.syllable_count > 0)
        .flat_map(|rp_a| {
            let parts_b = last_n_syllables_of_line(&b.words, rp_a.syllable_count);
            let syl_count = rp_a.syllable_count as f32;
            parts_b
                .into_iter()
                .map(move |b_phonemes| dl.distance(rp_a.phonemes, &b_phonemes) as f32 / syl_count)
        })
        .reduce(f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<Dictionary> = OnceLock::new();
    static DL: OnceLock<DamerauLevenshtein> = OnceLock::new();

    fn dict() -> &'static Dictionary {
        DICT.get_or_init(|| Dictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    fn dl() -> &'static DamerauLevenshtein {
        DL.get_or_init(DamerauLevenshtein::new)
    }

    fn ph(arpa: &[&str]) -> Vec<Phoneme> {
        arpa.iter().map(|s| s.parse().unwrap()).collect()
    }

    // --- rhyming_parts_of_last_word ---

    #[test]
    fn rhyming_parts_empty_line_returns_empty() {
        let line = Line::new("", dict());
        assert!(rhyming_parts_of_last_word(&line).is_empty());
    }

    #[test]
    fn rhyming_parts_unknown_word_returns_empty() {
        let line = Line::new("xyzzy", dict());
        assert!(rhyming_parts_of_last_word(&line).is_empty());
    }

    #[test]
    fn rhyming_parts_single_known_word() {
        // "cat" = K AE1 T, rhyming part starts at last stressed vowel AE1
        let line = Line::new("cat", dict());
        let parts = rhyming_parts_of_last_word(&line);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts.iter().next().unwrap().phonemes, ph(&["AE1", "T"]));
    }

    #[test]
    fn rhyming_parts_uses_last_word() {
        // "world" = W ER1 L D, rhyming part starts at ER1
        let line = Line::new("hello world", dict());
        let parts = rhyming_parts_of_last_word(&line);
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts.iter().next().unwrap().phonemes,
            ph(&["ER1", "L", "D"])
        );
    }

    #[test]
    fn rhyming_parts_multiple_distinct_parts() {
        // "contract" has two pronunciations with primary stress on different syllables:
        // K AA1 N T R AE2 K T → last primary stressed = AA1 → [AA1, N, T, R, AE2, K, T]
        // K AH0 N T R AE1 K T → last primary stressed = AE1 → [AE1, K, T]
        let line = Line::new("contract", dict());
        let parts = rhyming_parts_of_last_word(&line);
        assert_eq!(parts.len(), 2);
        let phoneme_sets: HashSet<&[Phoneme]> = parts.iter().map(|rp| rp.phonemes).collect();
        assert!(phoneme_sets.contains(ph(&["AA1", "N", "T", "R", "AE2", "K", "T"]).as_slice()));
        assert!(phoneme_sets.contains(ph(&["AE1", "K", "T"]).as_slice()));
    }

    #[test]
    fn rhyming_parts_multiple_pronunciations_deduplicated() {
        // "hello" has two pronunciations: HH AH0 L OW1 and HH EH0 L OW1
        // both share the same rhyming part [OW1], so the HashSet deduplicates to 1 entry
        let line = Line::new("hello", dict());
        let parts = rhyming_parts_of_last_word(&line);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts.iter().next().unwrap().phonemes, ph(&["OW1"]));
    }

    // --- last_n_syllables_of_line ---

    #[test]
    fn last_n_syllables_zero_returns_empty_vec() {
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 0);
        assert_eq!(result, HashSet::from([vec![]]));
    }

    #[test]
    fn last_n_syllables_within_last_word() {
        // "world" = W ER1 L D — last 1 syllable starts at ER1
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 1);
        assert_eq!(result, HashSet::from([ph(&["ER1", "L", "D"])]));
    }

    #[test]
    fn last_n_syllables_crosses_word_boundary() {
        // "world" has 1 syllable; need 1 more from "hello" (both pronunciations end in OW1)
        // result: [OW1] prepended to [W, ER1, L, D]
        let line = Line::new("hello world", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert_eq!(result, HashSet::from([ph(&["OW1", "W", "ER1", "L", "D"])]));
    }

    #[test]
    fn last_n_syllables_multiple_pronunciations_across_boundary() {
        // "contract fire": both words have multiple pronunciations, producing 4 combinations.
        // FIRE has two pronunciations:
        //   F AY1 ER0 (2 syllables) → needs 1 more from CONTRACT
        //   F AY1 R   (1 syllable)  → needs 2 more from CONTRACT
        // CONTRACT has two pronunciations:
        //   K AA1 N T R AE2 K T → last 1 syl = [AE2, K, T], last 2 syl = [AA1, N, T, R, AE2, K, T]
        //   K AH0 N T R AE1 K T → last 1 syl = [AE1, K, T], last 2 syl = [AH0, N, T, R, AE1, K, T]
        let line = Line::new("contract fire", dict());
        let result = last_n_syllables_of_line(&line.words, 3);
        assert_eq!(result.len(), 4);
        assert!(result.contains(&ph(&["AE2", "K", "T", "F", "AY1", "ER0"])));
        assert!(result.contains(&ph(&["AE1", "K", "T", "F", "AY1", "ER0"])));
        assert!(result.contains(&ph(&[
            "AA1", "N", "T", "R", "AE2", "K", "T", "F", "AY1", "R"
        ])));
        assert!(result.contains(&ph(&[
            "AH0", "N", "T", "R", "AE1", "K", "T", "F", "AY1", "R"
        ])));
    }

    #[test]
    fn last_n_syllables_unknown_word_returns_empty() {
        let line = Line::new("xyzzy", dict());
        let result = last_n_syllables_of_line(&line.words, 1);
        assert!(result.is_empty());
    }

    #[test]
    fn last_n_syllables_more_than_available_returns_empty() {
        // "cat" has 1 syllable, asking for 2 with no preceding words → empty
        let line = Line::new("cat", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn last_n_syllables_unknown_word_in_chain_returns_empty() {
        // "world" has 1 syllable, asking for 2 requires "xyzzy" which is unknown
        let line = Line::new("xyzzy world", dict());
        let result = last_n_syllables_of_line(&line.words, 2);
        assert!(result.is_empty());
    }

    // --- compare_rhyming_parts ---

    #[test]
    fn compare_rhyming_parts_identical_words_score_zero() {
        let a = Line::new("cat", dict());
        let b = Line::new("cat", dict());
        assert_eq!(compare_rhyming_parts(&a, &b, dl()), Some(0.0));
    }

    #[test]
    fn compare_rhyming_parts_rhyming_words_low_score() {
        // "cat" and "hat" rhyme — normalized score should be lower than a non-rhyme
        let cat = Line::new("cat", dict());
        let hat = Line::new("hat", dict());
        let non_rhyme = Line::new("hello", dict());
        let rhyme_score = compare_rhyming_parts(&cat, &hat, dl()).unwrap();
        let non_rhyme_score = compare_rhyming_parts(&cat, &non_rhyme, dl()).unwrap();
        assert!(rhyme_score < non_rhyme_score);
    }

    #[test]
    fn compare_rhyming_parts_unknown_word_returns_none() {
        let a = Line::new("xyzzy", dict());
        let b = Line::new("cat", dict());
        assert_eq!(compare_rhyming_parts(&a, &b, dl()), None);
        assert_eq!(compare_rhyming_parts(&b, &a, dl()), None);
    }

    #[test]
    fn compare_rhyming_parts_empty_line_returns_none() {
        let a = Line::new("", dict());
        let b = Line::new("cat", dict());
        assert_eq!(compare_rhyming_parts(&a, &b, dl()), None);
    }

    // --- Line ---

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
