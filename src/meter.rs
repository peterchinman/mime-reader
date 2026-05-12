use std::collections::HashSet;

use crate::error::ParseMeterError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SyllableStress {
    Stressed,
    Unstressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Meter {
    meter: Vec<SyllableStress>,
}

#[derive(Debug)]
struct PossibleMeters {
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

impl std::str::FromStr for PossibleMeters {
    type Err = ParseMeterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let segments = parse_segments(s)?;
        let possible_meters = expand_segments(segments);
        Ok(PossibleMeters { possible_meters })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(stresses: &[SyllableStress]) -> Meter {
        Meter {
            meter: stresses.to_vec(),
        }
    }

    const U: SyllableStress = SyllableStress::Unstressed;
    const S: SyllableStress = SyllableStress::Stressed;

    #[test]
    fn simple_meter_gives_one_possibility() {
        let pm: PossibleMeters = "x/x".parse().unwrap();
        assert_eq!(pm.possible_meters.len(), 1);
        assert!(pm.possible_meters.contains(&meter(&[U, S, U])));
    }

    #[test]
    fn optional_section_expands_to_multiple_possibilities() {
        let pm: PossibleMeters = "(x/)x/(x/)".parse().unwrap();
        // 3 possibilities here because 2 of the possibilites are  the same and set doesn't store duplicates
        assert_eq!(pm.possible_meters.len(), 3);
        assert!(pm.possible_meters.contains(&meter(&[U, S])));
        assert!(pm.possible_meters.contains(&meter(&[U, S, U, S])));
        assert!(pm.possible_meters.contains(&meter(&[U, S, U, S, U, S])));
    }

    #[test]
    fn complex_optional_gives_correct_count() {
        let pm: PossibleMeters = "(/x)/x (/)x/(x)".parse().unwrap();
        assert_eq!(pm.possible_meters.len(), 8);
        assert!(
            pm.possible_meters
                .contains(&meter(&[S, U, S, U, S, U, S, U]))
        );
    }

    #[test]
    fn unclosed_paren_returns_error() {
        let err = "x/x/(x/".parse::<PossibleMeters>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn unopened_paren_returns_error() {
        let err = "x/x/)x/".parse::<PossibleMeters>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn nested_parens_return_error() {
        let err = "x/x/(x/(x/))".parse::<PossibleMeters>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidParenNesting));
    }

    #[test]
    fn unrecognized_character_returns_error() {
        let err = "xjx".parse::<PossibleMeters>().unwrap_err();
        assert!(matches!(err, ParseMeterError::InvalidChar('j')));
    }
}
