use std::collections::HashSet;

use crate::{
    Stress,
    error::{
        MeterCheckError, ParseMeterError, ParseMeterSchemeError, ParseSyllableCountError,
        UnknownWordError,
    },
    line::{Line, WordEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyllableStress {
    Stressed,
    Unstressed,
    Either,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Meter {
    meter: Vec<SyllableStress>,
}

/// A parsed meter pattern, represented as the set of all concrete stress sequences it matches.
/// Optional sections (written with parentheses, e.g. `"(x/)x/"`) expand into multiple
/// possibilities.
#[derive(Debug)]
pub struct MeterSpecification {
    possible_meters: HashSet<Meter>,
}

enum Segment {
    Required(Vec<SyllableStress>),
    Optional(Vec<SyllableStress>),
}

fn parse_segments(s: &str) -> Result<Vec<Segment>, ParseMeterError> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut current: Vec<SyllableStress> = Vec::new();
    let mut open_col: Option<usize> = None;

    for (i, c) in s.chars().enumerate() {
        match c {
            c if c.is_whitespace() => continue,
            'x' => current.push(SyllableStress::Unstressed),
            '/' => current.push(SyllableStress::Stressed),
            '_' => current.push(SyllableStress::Either),
            '(' => {
                if open_col.is_some() {
                    return Err(ParseMeterError::InvalidParenNesting { col: i });
                }
                if !current.is_empty() {
                    segments.push(Segment::Required(std::mem::take(&mut current)));
                }
                open_col = Some(i);
            }
            ')' => {
                if open_col.is_none() {
                    return Err(ParseMeterError::InvalidParenNesting { col: i });
                }
                segments.push(Segment::Optional(std::mem::take(&mut current)));
                open_col = None;
            }
            _ => return Err(ParseMeterError::InvalidChar { c, col: i }),
        }
    }

    if let Some(col) = open_col {
        return Err(ParseMeterError::InvalidParenNesting { col });
    }

    if !current.is_empty() {
        segments.push(Segment::Required(current));
    }

    Ok(segments)
}

fn expand_segments(segments: Vec<Segment>) -> HashSet<Meter> {
    let mut paths: Vec<Meter> = vec![Meter { meter: Vec::new() }];

    for segment in segments {
        match segment {
            Segment::Required(stresses) => {
                for path in &mut paths {
                    path.meter.extend_from_slice(&stresses);
                }
            }
            Segment::Optional(stresses) => {
                let mut new_paths: Vec<Meter> = paths
                    .iter()
                    .map(|m| {
                        let mut extended = m.clone();
                        extended.meter.extend_from_slice(&stresses);
                        extended
                    })
                    .collect();
                paths.append(&mut new_paths);
            }
        }
    }

    paths.into_iter().collect()
}

impl std::str::FromStr for MeterSpecification {
    type Err = ParseMeterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let segments = parse_segments(s)?;
        let possible_meters = expand_segments(segments);
        Ok(MeterSpecification { possible_meters })
    }
}

impl MeterSpecification {
    fn from_syllable_range(min: usize, max: usize) -> Self {
        let possible_meters = (min..=max)
            .map(|n| Meter {
                meter: vec![SyllableStress::Either; n],
            })
            .collect();
        MeterSpecification { possible_meters }
    }

    /// Checks whether a Meter Spec matches a specific line. This is a very permissive check—we make no guesses about the Meter of the Line, instead we check every pronunciation of every word in the Line, and return true if any combinatoric possibility matches.
    pub fn validate_line(&self, line: &Line) -> Result<MeterMatchResult, MeterCheckError> {
        let meters: Vec<&[SyllableStress]> = self
            .possible_meters
            .iter()
            .map(|m| m.meter.as_slice())
            .collect();
        validate_recursive(0, &line.words, &meters)
    }
}

// Secondary stress is ambiguous if there is a primary stress either before or after it.
fn is_secondary_next_to_primary(stresses: &[Stress], i: usize) -> bool {
    stresses[i] == Stress::Secondary
        && (stresses.get(i + 1) == Some(&Stress::Primary)
            || i > 0 && stresses.get(i - 1) == Some(&Stress::Primary))
}

fn word_stresses_matches_meter(word_stresses: &[Stress], meter: &[SyllableStress]) -> bool {
    // Assumption: single-syllable words are inherently ambiguous and match any meter position
    if word_stresses.len() == 1 {
        return true;
    }
    word_stresses.iter().enumerate().all(|(i, s)| match s {
        // Either position matches any stress
        _ if meter[i] == SyllableStress::Either => true,
        // Assumption: primary stress must match a stressed position
        Stress::Primary => meter[i] == SyllableStress::Stressed,
        // Assumption: secondary stress adjacent to primary is ambiguous — matches any meter position
        Stress::Secondary if is_secondary_next_to_primary(word_stresses, i) => true,
        // Assumption: non-ambiguous secondary stress must match a stressed position
        Stress::Secondary => meter[i] == SyllableStress::Stressed,
        // Assumption: unstressed syllables must match unstressed positions
        Stress::Unstressed => meter[i] == SyllableStress::Unstressed,
    })
}

#[derive(Debug, PartialEq)]
pub enum MeterMatchResult {
    Match,
    NoMatch { reasons: HashSet<MeterMismatch> },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeterMismatch {
    MeterTooShort {
        word_index_failed_at: usize,
    },
    MeterTooLong {
        remaining_syllables_in_meter: usize,
    },
    WordsStressMismatch {
        expected_stress_pattern_of_word: Vec<SyllableStress>,
        word_index_failed_at: usize,
    },
}

/// Recursively matches the pronunciations of the next word in a sequuence of words against a collection of valid meters. Returns true if it finds any branch that matches, false if all branches fail.
fn validate_recursive(
    word_index_accumulator: usize,
    words: &[WordEntry],
    meters: &[&[SyllableStress]],
) -> Result<MeterMatchResult, MeterCheckError> {
    // Base-case: if we've chewed thru all the words, and if any of the valid meters is also empty, we've found a match.
    if words.is_empty() {
        return if meters.iter().any(|m| m.is_empty()) {
            Ok(MeterMatchResult::Match)
        } else {
            Ok(MeterMatchResult::NoMatch {
                reasons: meters
                    .iter()
                    .map(|m| MeterMismatch::MeterTooLong {
                        remaining_syllables_in_meter: m.len(),
                    })
                    .collect(),
            })
        };
    }

    if meters.is_empty() || meters.iter().all(|m| m.is_empty()) {
        return Ok(MeterMatchResult::NoMatch {
            reasons: HashSet::from([MeterMismatch::MeterTooShort {
                word_index_failed_at: word_index_accumulator,
            }]),
        });
    }

    // Branch out on every pronunciation of the next word.
    //
    // TODO: is there some way of failing a branch early, rather than checking every combinatoric possiblity?
    // One possibility would be storing on WordEntry the shortest and longest syllable length of its pronunciations, so that we can pre-emptively check whether our Line is too long or too short.

    let next_word = &words[0];
    let Some(next_word_possible_stresses) = next_word.data.stress_patterns() else {
        return Err(MeterCheckError::UnknownWord(UnknownWordError {
            word: next_word.word.clone(),
        }));
    };

    let mut failure_reasons = HashSet::new();

    for next_word_stresses in next_word_possible_stresses {
        let mut meter_match_suffixes: Vec<&[SyllableStress]> = Vec::new();
        for m in meters {
            // Enough syllables in the meter to match the next word?
            if next_word_stresses.len() > m.len() {
                failure_reasons.insert(MeterMismatch::MeterTooShort {
                    word_index_failed_at: word_index_accumulator,
                });
                continue;
            }
            // Does the meter match the next word's stresses?
            if word_stresses_matches_meter(next_word_stresses, &m[..next_word_stresses.len()]) {
                meter_match_suffixes.push(&m[next_word_stresses.len()..]);
            } else {
                failure_reasons.insert(MeterMismatch::WordsStressMismatch {
                    expected_stress_pattern_of_word: m[..next_word_stresses.len()].to_vec(),
                    word_index_failed_at: word_index_accumulator,
                });
            }
        }

        // If there are meter match suffixes, recursively validate the rest of the line.
        if !meter_match_suffixes.is_empty() {
            match validate_recursive(
                word_index_accumulator + 1,
                &words[1..],
                &meter_match_suffixes,
            )? {
                MeterMatchResult::Match => return Ok(MeterMatchResult::Match),
                MeterMatchResult::NoMatch {
                    reasons: branch_reasons,
                } => failure_reasons.extend(branch_reasons),
            }
        }
    }

    Ok(MeterMatchResult::NoMatch {
        reasons: failure_reasons,
    })
}

#[derive(Debug)]
pub struct SyllableCountSpecification(MeterSpecification);

impl std::str::FromStr for SyllableCountSpecification {
    type Err = ParseSyllableCountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((min_str, max_str)) = s.split_once('-') {
            let min = min_str
                .trim()
                .parse::<usize>()
                .map_err(|_| ParseSyllableCountError::InvalidNumber)?;
            let max = max_str
                .trim()
                .parse::<usize>()
                .map_err(|_| ParseSyllableCountError::InvalidNumber)?;
            if min > max {
                return Err(ParseSyllableCountError::InvalidRange);
            }
            Ok(Self(MeterSpecification::from_syllable_range(min, max)))
        } else {
            let n = s
                .trim()
                .parse::<usize>()
                .map_err(|_| ParseSyllableCountError::InvalidNumber)?;
            Ok(Self(MeterSpecification::from_syllable_range(n, n)))
        }
    }
}

impl SyllableCountSpecification {
    pub fn validate_line(&self, line: &Line) -> Result<MeterMatchResult, MeterCheckError> {
        self.0.validate_line(line)
    }
}

#[derive(Debug)]
pub struct MeterScheme {
    scheme: Vec<MeterSpecification>,
}

impl std::str::FromStr for MeterScheme {
    type Err = ParseMeterSchemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scheme = s
            .lines()
            .enumerate()
            .map(|(line, text)| {
                text.parse::<MeterSpecification>()
                    .map_err(|source| ParseMeterSchemeError { line, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MeterScheme { scheme })
    }
}

impl MeterScheme {
    /// Returns the meter specification for the given line index, wrapping around the scheme if necessary.
    fn line_specification(&self, line_index: usize) -> &MeterSpecification {
        &self.scheme[line_index % self.scheme.len()]
    }

    pub fn check_line(
        &self,
        lines: &[Line],
        target_index: usize,
    ) -> Result<MeterMatchResult, MeterCheckError> {
        if target_index >= lines.len() {
            return Err(MeterCheckError::TargetLineOutOfBounds {
                target_index,
                line_count: lines.len(),
            });
        }
        self.line_specification(target_index)
            .validate_line(&lines[target_index])
    }
}

#[cfg(test)]
mod tests {
    use crate::{Dictionary, line::Line};
    use std::sync::OnceLock;

    use super::*;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<Dictionary> = OnceLock::new();

    fn dict() -> &'static Dictionary {
        DICT.get_or_init(|| Dictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    fn meter(stresses: &[SyllableStress]) -> Meter {
        Meter {
            meter: stresses.to_vec(),
        }
    }

    const U: SyllableStress = SyllableStress::Unstressed;
    const S: SyllableStress = SyllableStress::Stressed;
    const E: SyllableStress = SyllableStress::Either;

    fn assert_meter_matches(result: Result<MeterMatchResult, MeterCheckError>) {
        assert!(matches!(result, Ok(MeterMatchResult::Match)), "{result:?}");
    }

    fn assert_meter_no_match(
        result: Result<MeterMatchResult, MeterCheckError>,
    ) -> HashSet<MeterMismatch> {
        let Ok(MeterMatchResult::NoMatch { reasons }) = result else {
            panic!("expected NoMatch, got {result:?}");
        };
        reasons
    }

    #[test]
    fn simple_meter_gives_one_possibility() {
        let pm: MeterSpecification = "x/x".parse().unwrap();
        assert_eq!(pm.possible_meters.len(), 1);
        assert!(pm.possible_meters.contains(&meter(&[U, S, U])));
    }

    #[test]
    fn optional_section_expands_to_multiple_possibilities() {
        let pm: MeterSpecification = "(x/)x/(x/)".parse().unwrap();
        // 3 possibilities here because 2 of the possibilites are the same and set doesn't store duplicates
        assert_eq!(pm.possible_meters.len(), 3);
        assert!(pm.possible_meters.contains(&meter(&[U, S])));
        assert!(pm.possible_meters.contains(&meter(&[U, S, U, S])));
        assert!(pm.possible_meters.contains(&meter(&[U, S, U, S, U, S])));
    }

    #[test]
    fn complex_optional_gives_correct_count() {
        let pm: MeterSpecification = "(/x)/x (/)x/(x)".parse().unwrap();
        assert_eq!(pm.possible_meters.len(), 8);
        assert!(
            pm.possible_meters
                .contains(&meter(&[S, U, S, U, S, U, S, U]))
        );
    }

    #[test]
    fn unclosed_paren_returns_error() {
        let err = "x/x/(x/".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(
            err,
            ParseMeterError::InvalidParenNesting { col: 4 }
        ));
    }

    #[test]
    fn unopened_paren_returns_error() {
        let err = "x/x/)x/".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(
            err,
            ParseMeterError::InvalidParenNesting { col: 4 }
        ));
    }

    #[test]
    fn nested_parens_return_error() {
        let err = "x/x/(x/(x/))".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(
            err,
            ParseMeterError::InvalidParenNesting { col: 7 }
        ));
    }

    #[test]
    fn unrecognized_character_returns_error() {
        let err = "xjx".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(
            err,
            ParseMeterError::InvalidChar { c: 'j', col: 1 }
        ));
        assert_eq!(err.to_string(), "invalid stress character 'j' at column 2");
    }

    // --- wildcard (_) ---

    #[test]
    fn wildcard_parses_to_either() {
        let pm: MeterSpecification = "x_/".parse().unwrap();
        assert!(pm.possible_meters.contains(&meter(&[U, E, S])));
    }

    #[test]
    fn wildcard_matches_unstressed_position() {
        // HELLO: AH0 L OW1
        let line = Line::new("hello", dict());
        let spec: MeterSpecification = "_/".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn wildcard_matches_stressed_position() {
        // KARAOKE: EH2 R IY0 OW1 K IY0
        let line = Line::new("karaoke", dict());
        let spec: MeterSpecification = "/x_x".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn wildcard_wrong_length_fails() {
        let line = Line::new("hello", dict());
        let spec: MeterSpecification = "_/_".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    // --- check_meter_validity: single syllable words ---

    #[test]
    fn single_syllable_iambic_matches() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x/".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn single_syllable_short_meter_fails() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    #[test]
    fn single_syllable_long_meter_fails() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x/x".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    // --- check_meter_validity: multi-syllable words ---
    // KARAOKE  K EH2 R IY0 OW1 K IY0
    // OKEY-DOKEY  OW1 K IY0 D OW1 K IY0

    #[test]
    fn multisyllable_good_meter_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x /x/x".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn multisyllable_bad_meter_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "x/x/ x/x/".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    #[test]
    fn multisyllable_short_meter_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x /x/".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    // --- check_meter_validity: optional meter ---

    #[test]
    fn optional_meter_good_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x (/)x/x".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn optional_meter_good2_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "(/x)/x (/)x/(x)".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn optional_meter_bad_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "x/x(/ x)/x/".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    // --- check_meter_validity: stress-shifting words ---
    // fire, conflicts, content, record can each stress differently

    #[test]
    fn stress_shifting_good_meter_matches() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/ /x /x /x".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn stress_shifting_good_meter2_matches() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/x x/ /x x/".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn stress_shifting_bad_meter_fails() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/(x) x/ x/ xx".parse().unwrap();
        assert_meter_no_match(spec.validate_line(&line));
    }

    // --- MeterMatchResult mismatch reasons ---

    #[test]
    fn validate_recursive_reports_meter_too_long_when_words_are_exhausted() {
        let meter_a = [U, S];
        let meter_b = [S];
        let meters: Vec<&[SyllableStress]> = vec![&meter_a, &meter_b];
        let words: Vec<WordEntry> = vec![];

        let reasons = assert_meter_no_match(validate_recursive(0, &words, &meters));

        assert!(reasons.contains(&MeterMismatch::MeterTooLong {
            remaining_syllables_in_meter: 2,
        }));
        assert!(reasons.contains(&MeterMismatch::MeterTooLong {
            remaining_syllables_in_meter: 1,
        }));
    }

    #[test]
    fn validate_recursive_reports_meter_too_short_when_word_exceeds_meter() {
        let line = Line::new("hello", dict());
        let meter = [U];
        let meters: Vec<&[SyllableStress]> = vec![&meter];

        let reasons = assert_meter_no_match(validate_recursive(0, &line.words, &meters));

        assert!(reasons.contains(&MeterMismatch::MeterTooShort {
            word_index_failed_at: 0,
        }));
    }

    #[test]
    fn validate_recursive_reports_word_stress_mismatch() {
        let line = Line::new("hello", dict());
        let meter = [S, S];
        let meters: Vec<&[SyllableStress]> = vec![&meter];

        let reasons = assert_meter_no_match(validate_recursive(0, &line.words, &meters));

        assert!(reasons.contains(&MeterMismatch::WordsStressMismatch {
            expected_stress_pattern_of_word: vec![S, S],
            word_index_failed_at: 0,
        }));
    }

    #[test]
    fn validate_recursive_accumulates_reasons_from_multiple_failed_branches() {
        let line = Line::new("hello world", dict());
        let meter_that_matches_first_word_then_runs_out = [U, S];
        let meter_that_fails_first_word_stress = [S, S];
        let meters: Vec<&[SyllableStress]> = vec![
            &meter_that_matches_first_word_then_runs_out,
            &meter_that_fails_first_word_stress,
        ];

        let reasons = assert_meter_no_match(validate_recursive(0, &line.words, &meters));

        assert!(reasons.contains(&MeterMismatch::MeterTooShort {
            word_index_failed_at: 1,
        }));
        assert!(reasons.contains(&MeterMismatch::WordsStressMismatch {
            expected_stress_pattern_of_word: vec![S, S],
            word_index_failed_at: 0,
        }));
    }

    // --- SyllableCountSpecification ---

    #[test]
    fn syllable_count_parse_exact() {
        let spec: SyllableCountSpecification = "8".parse().unwrap();
        // "I want to suck your blood right now" = 8 single-syllable words
        let line = Line::new("I want to suck your blood right now", dict());
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn syllable_count_parse_range_matches() {
        let spec: SyllableCountSpecification = "6-10".parse().unwrap();
        let line = Line::new("I want to suck your blood right now", dict());
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn syllable_count_below_range_fails() {
        let spec: SyllableCountSpecification = "9-12".parse().unwrap();
        let line = Line::new("I want to suck your blood right now", dict());
        assert_meter_no_match(spec.validate_line(&line));
    }

    #[test]
    fn syllable_count_above_range_fails() {
        let spec: SyllableCountSpecification = "4-7".parse().unwrap();
        let line = Line::new("I want to suck your blood right now", dict());
        assert_meter_no_match(spec.validate_line(&line));
    }

    #[test]
    fn syllable_count_multisyllable_words() {
        // karaoke (4) + okey-dokey (4) = 8
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: SyllableCountSpecification = "8".parse().unwrap();
        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn syllable_count_variable_syllable_word() {
        // "fire" has pronunciations with 1 and 2 syllables
        let line = Line::new("fire", dict());
        assert_meter_matches(
            "1".parse::<SyllableCountSpecification>()
                .unwrap()
                .validate_line(&line),
        );
        assert_meter_matches(
            "2".parse::<SyllableCountSpecification>()
                .unwrap()
                .validate_line(&line),
        );
    }

    #[test]
    fn syllable_count_unknown_word_fails() {
        let line = Line::new("hello xyzzy", dict());
        let spec: SyllableCountSpecification = "1-10".parse().unwrap();
        assert_eq!(
            spec.validate_line(&line),
            Err(MeterCheckError::UnknownWord(UnknownWordError {
                word: "xyzzy".to_string(),
            }))
        );
    }

    #[test]
    fn syllable_count_invalid_number_returns_error() {
        let err = "abc".parse::<SyllableCountSpecification>().unwrap_err();
        assert!(matches!(err, ParseSyllableCountError::InvalidNumber));
    }

    #[test]
    fn syllable_count_inverted_range_returns_error() {
        let err = "11-8".parse::<SyllableCountSpecification>().unwrap_err();
        assert!(matches!(err, ParseSyllableCountError::InvalidRange));
    }

    // --- MeterScheme ---

    #[test]
    fn meter_scheme_parses_multiple_lines() {
        let ms: MeterScheme = "x/x/x/x/x/\n//xx//".parse().unwrap();
        assert_eq!(ms.scheme.len(), 2);
        assert!(
            ms.scheme[0]
                .possible_meters
                .contains(&meter(&[U, S, U, S, U, S, U, S, U, S]))
        );
        assert!(
            ms.scheme[1]
                .possible_meters
                .contains(&meter(&[S, S, U, U, S, S]))
        );
    }

    #[test]
    fn meter_scheme_line_specification_wraps_by_line_index() {
        let ms: MeterScheme = "x/\n/x".parse().unwrap();

        assert!(
            ms.line_specification(0)
                .possible_meters
                .contains(&meter(&[U, S]))
        );
        assert!(
            ms.line_specification(1)
                .possible_meters
                .contains(&meter(&[S, U]))
        );
        assert!(
            ms.line_specification(2)
                .possible_meters
                .contains(&meter(&[U, S]))
        );
        assert!(
            ms.line_specification(3)
                .possible_meters
                .contains(&meter(&[S, U]))
        );
    }

    #[test]
    fn meter_scheme_propagates_line_parse_error() {
        let err = "x/x/\nx/j/".parse::<MeterScheme>().unwrap_err();
        assert_eq!(err.line, 1);
        assert!(matches!(
            err.source,
            ParseMeterError::InvalidChar { c: 'j', col: 2 }
        ));
    }
}
