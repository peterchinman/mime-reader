use std::collections::HashSet;

use crate::{
    Line, Phoneme, Stress,
    error::{MeterMatchError, ParseMeterError},
    line::WordEntry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SyllableStress {
    Stressed,
    Unstressed,
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
    let mut in_optional = false;

    for c in s.chars() {
        match c {
            c if c.is_whitespace() => continue,
            'x' | '/' => {
                let stress = match c {
                    'x' => SyllableStress::Unstressed,
                    _ => SyllableStress::Stressed,
                };
                current.push(stress);
            }
            '(' => {
                if in_optional {
                    return Err(ParseMeterError::InvalidParenNesting);
                }
                if !current.is_empty() {
                    segments.push(Segment::Required(current));
                    current = Vec::new();
                }
                in_optional = true;
            }
            ')' => {
                if !in_optional {
                    return Err(ParseMeterError::InvalidParenNesting);
                }
                segments.push(Segment::Optional(current));
                current = Vec::new();
                in_optional = false;
            }
            _ => return Err(ParseMeterError::InvalidChar(c)),
        }
    }

    if in_optional {
        return Err(ParseMeterError::InvalidParenNesting);
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
    pub fn matches(&self, line: &crate::line::Line) -> bool {
        let meters: Vec<&[SyllableStress]> = self
            .possible_meters
            .iter()
            .map(|m| m.meter.as_slice())
            .collect();
        matches_recursive(&line.words, &meters).is_ok()
    }
}

fn is_ambiguous_secondary(stresses: &[Stress], i: usize) -> bool {
    stresses[i] == Stress::Secondary
        && (stresses.get(i + 1) == Some(&Stress::Primary)
            || i > 0 && stresses.get(i - 1) == Some(&Stress::Primary))
}

// TODO return helpful error instead of bool
fn matches_recursive(
    words: &[WordEntry],
    meters: &[&[SyllableStress]],
) -> Result<(), MeterMatchError> {
    if words.is_empty() {
        return if meters.iter().any(|m| m.is_empty()) {
            Ok(())
        } else {
            Err(MeterMatchError::FailedMatch)
        };
    }
    let next_word_possible_stresses = words
        .first()
        .ok_or(MeterMatchError::FailedMatch)?
        .pronunciations
        .as_ref()
        .ok_or(MeterMatchError::FailedMatch)?
        .iter()
        .map(|pronunciation| {
            pronunciation
                .iter()
                .filter_map(|phoneme| match phoneme {
                    Phoneme::Consonant(_) => None,
                    Phoneme::Vowel(vowel) => Some(vowel.stress),
                })
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>();

    for stresses in next_word_possible_stresses {
        // We want to check if any pronunciation of the next word matches any remaining meter, and if so, to recurse with those matches
        let mut meter_match_suffixes: Vec<&[SyllableStress]> = Vec::new();
        for m in meters {
            if stresses.len() > m.len() {
                continue;
            }
            if stresses.len() == 1 {
                meter_match_suffixes.push(&m[1..]);
                continue;
            } else {
                let valid = stresses.iter().enumerate().all(|(i, s)| match s {
                    Stress::Primary => m[i] == SyllableStress::Stressed,
                    Stress::Secondary if is_ambiguous_secondary(&stresses, i) => true,
                    Stress::Secondary => m[i] == SyllableStress::Stressed,
                    Stress::Unstressed => m[i] == SyllableStress::Unstressed,
                });
                if valid {
                    meter_match_suffixes.push(&m[stresses.len()..]);
                }
            }
        }

        if !meter_match_suffixes.is_empty() {
            if matches_recursive(&words[1..], &meter_match_suffixes).is_ok() {
                return Ok(());
            }
        }
    }

    Err(MeterMatchError::FailedMatch)
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
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn unopened_paren_returns_error() {
        let err = "x/x/)x/".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn nested_parens_return_error() {
        let err = "x/x/(x/(x/))".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn unrecognized_character_returns_error() {
        let err = "xjx".parse::<MeterSpecification>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidChar('j')));
    }

    // --- check_meter_validity: single syllable words ---

    #[test]
    fn single_syllable_iambic_matches() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x/".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn single_syllable_short_meter_fails() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x".parse().unwrap();
        assert!(!spec.matches(&line));
    }

    #[test]
    fn single_syllable_long_meter_fails() {
        let line = Line::new("I want to suck your blood right now", dict());
        let spec: MeterSpecification = "x/x/x/x/x".parse().unwrap();
        assert!(!spec.matches(&line));
    }

    // --- check_meter_validity: multi-syllable words ---
    // KARAOKE  K EH2 R IY0 OW1 K IY0
    // OKEY-DOKEY  OW1 K IY0 D OW1 K IY0

    #[test]
    fn multisyllable_good_meter_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x /x/x".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn multisyllable_bad_meter_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "x/x/ x/x/".parse().unwrap();
        assert!(!spec.matches(&line));
    }

    #[test]
    fn multisyllable_short_meter_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x /x/".parse().unwrap();
        assert!(!spec.matches(&line));
    }

    // --- check_meter_validity: optional meter ---

    #[test]
    fn optional_meter_good_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "/x/x (/)x/x".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn optional_meter_good2_matches() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "(/x)/x (/)x/(x)".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn optional_meter_bad_fails() {
        let line = Line::new("karaoke okey-dokey", dict());
        let spec: MeterSpecification = "x/x(/ x)/x/".parse().unwrap();
        assert!(!spec.matches(&line));
    }

    // --- check_meter_validity: stress-shifting words ---
    // fire, conflicts, content, record can each stress differently

    #[test]
    fn stress_shifting_good_meter_matches() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/ /x /x /x".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn stress_shifting_good_meter2_matches() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/x x/ /x x/".parse().unwrap();
        assert!(spec.matches(&line));
    }

    #[test]
    fn stress_shifting_bad_meter_fails() {
        let line = Line::new("fire conflicts content record", dict());
        let spec: MeterSpecification = "/(x) x/ x/ xx".parse().unwrap();
        assert!(!spec.matches(&line));
    }
}
