use std::collections::{HashMap, HashSet};

use crate::{
    DamerauLevenshtein, Line,
    error::ParseRhymeError,
    line::{WordData, WordEntry},
    phoneme::{Phoneme, RhymingPart, get_last_n_syllables, get_rhyming_part},
};

pub struct RhymeScheme {
    scheme: Vec<SchemeRole>,
    threshold: f32,
}

/// A role within the stored rhyme scheme pattern.
///
/// Positions here are scheme-relative: for example, in `ABAB`, the third
/// position is a follower whose `leader_position` is `0`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SchemeRole {
    /// The first occurrence of a rhyme label within the scheme pattern.
    Leader,
    /// A scheme position that imposes no rhyme constraint, represented by `_`.
    Unconstrained,
    /// A repeated rhyme label that should rhyme with an earlier scheme position.
    Follower {
        /// The index within the scheme pattern of the label's first occurrence.
        leader_position: usize,
    },
}

/// A role for a concrete line in a poem after resolving the repeated scheme.
///
/// Positions here are poem-line-relative: for example, with repeated `ABAB`,
/// line `6` is a follower whose `leader_line` is `4`.
#[derive(Debug, PartialEq)]
enum LineRole {
    /// This line establishes the rhyme for its group in the current scheme cycle.
    Leader,
    /// This line has no rhyme constraint.
    Unconstrained,
    /// This line should rhyme with an earlier concrete line.
    Follower {
        /// The absolute line index of the leader this line should rhyme with.
        leader_line: usize,
    },
}

#[derive(Debug, PartialEq)]
pub enum RhymeCheckResult {
    Leader,
    Unconstrained,
    Follower {
        leader_line: usize,
        /// The distance between the leader's rhyming part and the follower's rhyming part. None if the distance is unknown.
        distance: Option<f32>,
        passed: Option<bool>,
    },
}

impl RhymeScheme {
    pub fn new(s: &str, threshold: f32) -> Result<Self, ParseRhymeError> {
        let mut map: HashMap<char, usize> = HashMap::new();
        let scheme = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .enumerate()
            .map(|(i, c)| match c {
                '_' => SchemeRole::Unconstrained,
                _ => match map.get(&c) {
                    Some(&leader_position) => SchemeRole::Follower { leader_position },
                    None => {
                        map.insert(c, i);
                        SchemeRole::Leader
                    }
                },
            })
            .collect();
        Ok(RhymeScheme { scheme, threshold })
    }

    /// Classifies a line's role in the rhyme scheme:
    /// - `Unconstrained`: the scheme doesn't require this line to rhyme with anything
    /// - `Leader`: this line is the first occurrence of its rhyme group in its cycle
    /// - `Follower`: this line should rhyme with the line at the given leader index
    fn line_role(&self, line_index: usize) -> LineRole {
        let pattern_len = self.scheme.len();
        let line_position = line_index % pattern_len;
        let cycle_start = line_index - line_position;

        match self.scheme[line_position] {
            SchemeRole::Leader => LineRole::Leader,
            SchemeRole::Unconstrained => LineRole::Unconstrained,
            SchemeRole::Follower { leader_position } => LineRole::Follower {
                leader_line: cycle_start + leader_position,
            },
        }
    }

    /// Checks the Line at the given Target Index. Returns a [`RhymeCheckResult`] indicating whether the line is a Leader, Unconstrained, or a Follower.
    /// If the line is a Follower, returns the minimum { distance between (all of) the (possible) rhyming part(s) of the last word of the Leader against the same sized portion of Target, divided by the syllable count } and checks whether this is below the threshold defined in the [`RhymeScheme`].
    pub fn check_line(
        &self,
        lines: &[Line],
        target_index: usize,
        dl: &DamerauLevenshtein,
    ) -> RhymeCheckResult {
        match self.line_role(target_index) {
            LineRole::Leader => RhymeCheckResult::Leader,
            LineRole::Unconstrained => RhymeCheckResult::Unconstrained,
            LineRole::Follower { leader_line } => {
                let distance = compare_rhyming_parts(&lines[leader_line], &lines[target_index], dl);
                let passed = distance.map(|d| d <= self.threshold);
                RhymeCheckResult::Follower {
                    leader_line,
                    distance,
                    passed,
                }
            }
        }
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
fn compare_rhyming_parts(a: &Line, b: &Line, dl: &DamerauLevenshtein) -> Option<f32> {
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
    use crate::Dictionary;
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

    fn pronunciation(arpa: &[&str]) -> crate::line::Pronunciation {
        let phonemes = ph(arpa);
        let stress_pattern = phonemes
            .iter()
            .filter_map(|p| match p {
                Phoneme::Vowel(v) => Some(v.stress),
                Phoneme::Consonant(_) => None,
            })
            .collect();

        crate::line::Pronunciation {
            phonemes: phonemes.into_boxed_slice(),
            stress_pattern,
        }
    }

    fn known_word(word: &str, pronunciations: &[&[&str]]) -> WordEntry {
        WordEntry {
            word: word.to_string(),
            data: WordData::Known(
                pronunciations
                    .iter()
                    .map(|arpa| pronunciation(arpa))
                    .collect(),
            ),
        }
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

    #[test]
    fn compare_rhyming_parts_finds_longer_match_across_follower_pronunciation_combinations() {
        let leader = Line {
            words: vec![known_word(
                "leader",
                &[&["AE1", "T"][..], &["IY1", "Z", "AH0"][..]],
            )],
        };
        let follower = Line {
            words: vec![
                known_word("follower_prefix", &[&["AA1", "R"][..], &["IY1", "Z"][..]]),
                known_word("follower_last", &[&["EH0"][..], &["AH0"][..]]),
            ],
        };

        assert_eq!(compare_rhyming_parts(&leader, &follower, dl()), Some(0.0));
    }

    #[test]
    fn compare_rhyming_parts_finds_shorter_match_across_follower_pronunciation_combinations() {
        let leader = Line {
            words: vec![known_word(
                "leader",
                &[
                    &["AE1", "T", "AH0"][..],
                    &["IY1", "Z", "AH0", "N", "OW0"][..],
                ],
            )],
        };
        let follower = Line {
            words: vec![
                known_word(
                    "follower_prefix",
                    &[&["OW1", "K", "ER0"][..], &["UH0", "M", "AE1", "T"][..]],
                ),
                known_word("follower_last", &[&["IH0"][..], &["AH0"][..]]),
            ],
        };

        assert_eq!(compare_rhyming_parts(&leader, &follower, dl()), Some(0.0));
    }

    // --- RhymeScheme::new ---

    #[test]
    fn rhyme_scheme_basic_pattern() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_ignores_whitespace() {
        let rs = RhymeScheme::new("A B\nA B", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_underscore_is_unconstrained() {
        let rs = RhymeScheme::new("A_BA", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Unconstrained,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_repeated_letters_follow_first_appearance() {
        let rs = RhymeScheme::new("ZAZA", 8.0).unwrap();
        assert_eq!(
            rs.scheme,
            vec![
                SchemeRole::Leader,
                SchemeRole::Leader,
                SchemeRole::Follower { leader_position: 0 },
                SchemeRole::Follower { leader_position: 1 }
            ]
        );
    }

    #[test]
    fn rhyme_scheme_threshold_stored() {
        let rs = RhymeScheme::new("AABB", 7.5).unwrap();
        assert_eq!(rs.threshold, 7.5);
    }

    // --- line_role ---

    #[test]
    fn line_role_first_occurrence_is_leader() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(rs.line_role(0), LineRole::Leader);
        assert_eq!(rs.line_role(1), LineRole::Leader);
    }

    #[test]
    fn line_role_follower_points_to_leader() {
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        assert_eq!(rs.line_role(2), LineRole::Follower { leader_line: 0 });
        assert_eq!(rs.line_role(3), LineRole::Follower { leader_line: 1 });
    }

    #[test]
    fn line_role_resets_per_cycle() {
        // Each repetition of the pattern is its own group of leaders/followers.
        let rs = RhymeScheme::new("ABAB", 8.0).unwrap();
        // line 4 starts cycle 2; it's the new leader for its A group
        assert_eq!(rs.line_role(4), LineRole::Leader);
        assert_eq!(rs.line_role(5), LineRole::Leader);
        // line 6 is the second A of cycle 2 → leader is line 4
        assert_eq!(rs.line_role(6), LineRole::Follower { leader_line: 4 });
        assert_eq!(rs.line_role(7), LineRole::Follower { leader_line: 5 });
    }

    #[test]
    fn line_role_unconstrained_returns_unconstrained() {
        let rs = RhymeScheme::new("A_BA", 8.0).unwrap();
        assert_eq!(rs.line_role(1), LineRole::Unconstrained);
    }

    // --- RhymeScheme::check_line ---

    #[test]
    fn check_line_leader_returns_leader() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("dog", dict())];

        assert_eq!(rs.check_line(&lines, 0, dl()), RhymeCheckResult::Leader);
        assert_eq!(rs.check_line(&lines, 1, dl()), RhymeCheckResult::Leader);
    }

    #[test]
    fn check_line_unconstrained_returns_unconstrained() {
        let rs = RhymeScheme::new("A_", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("orange", dict())];

        assert_eq!(
            rs.check_line(&lines, 1, dl()),
            RhymeCheckResult::Unconstrained
        );
    }

    #[test]
    fn check_line_follower_uses_leader_line() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![
            Line::new("cat", dict()),
            Line::new("dog", dict()),
            Line::new("hat", dict()),
            Line::new("fog", dict()),
        ];

        assert_eq!(
            rs.check_line(&lines, 2, dl()),
            RhymeCheckResult::Follower {
                leader_line: 0,
                distance: Some(0.0),
                passed: Some(true),
            }
        );
        assert_eq!(
            rs.check_line(&lines, 3, dl()),
            RhymeCheckResult::Follower {
                leader_line: 1,
                distance: Some(0.0),
                passed: Some(true),
            }
        );
    }

    #[test]
    fn check_line_follower_resets_leader_per_cycle() {
        let rs = RhymeScheme::new("ABAB", 1.0).unwrap();
        let lines = vec![
            Line::new("cat", dict()),
            Line::new("dog", dict()),
            Line::new("hat", dict()),
            Line::new("fog", dict()),
            Line::new("moon", dict()),
            Line::new("light", dict()),
            Line::new("tune", dict()),
            Line::new("night", dict()),
        ];

        assert_eq!(
            rs.check_line(&lines, 6, dl()),
            RhymeCheckResult::Follower {
                leader_line: 4,
                distance: Some(0.0),
                passed: Some(true),
            }
        );
        assert_eq!(
            rs.check_line(&lines, 7, dl()),
            RhymeCheckResult::Follower {
                leader_line: 5,
                distance: Some(0.0),
                passed: Some(true),
            }
        );
    }

    #[test]
    fn check_line_follower_unknown_word_returns_unknown_distance() {
        let rs = RhymeScheme::new("AA", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("xyzzy", dict())];

        assert_eq!(
            rs.check_line(&lines, 1, dl()),
            RhymeCheckResult::Follower {
                leader_line: 0,
                distance: None,
                passed: None,
            }
        );
    }

    #[test]
    fn check_line_follower_empty_line_returns_unknown_distance() {
        let rs = RhymeScheme::new("AA", 1.0).unwrap();
        let lines = vec![Line::new("cat", dict()), Line::new("", dict())];

        assert_eq!(
            rs.check_line(&lines, 1, dl()),
            RhymeCheckResult::Follower {
                leader_line: 0,
                distance: None,
                passed: None,
            }
        );
    }
}
