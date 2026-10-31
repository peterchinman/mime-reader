use std::collections::HashSet;

use crate::{
    Stress,
    error::{
        MeterCheckError, ParseMeterError, ParseMeterSchemeError, ParseSyllableCountError,
        ParseSyllableCountSchemeError, UnknownWordError,
    },
    line::{Line, WordData, WordEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// Checks a line against the meter. This is a very permissive check—we make no guesses about the Meter of the Line, instead we try every pronunciation of every word and every optional section of the meter, and judge the line by the reading that deviates least.
    pub fn validate_line(&self, line: &Line) -> Result<MeterMatchResult, MeterCheckError> {
        Ok(MeterMatchResult::from_deviations(
            self.deviations_for_line(line)?,
            0,
        ))
    }

    /// Returns the deviations of the line's least-deviating reading: its choice of
    /// pronunciations, optional-meter path, and alignment of syllables with the
    /// meter. The analysis continues after a mismatch, so every wrong stress,
    /// extra syllable, and missing syllable is recorded. An empty result means the
    /// line fits.
    pub fn deviations_for_line(&self, line: &Line) -> Result<Vec<MeterDeviation>, MeterCheckError> {
        let stress_patterns = line
            .words
            .iter()
            .map(distinct_stress_patterns)
            .collect::<Result<Vec<_>, _>>()?;
        let mut meters: Vec<&Meter> = self.possible_meters.iter().collect();
        meters.sort_by(|a, b| a.meter.cmp(&b.meter));

        // The first meter path with the fewest deviations.
        let mut best: Option<(usize, Alignment<'_>)> = None;
        for meter in meters {
            let mut alignment = Alignment::new(&stress_patterns, &meter.meter);
            let cost = alignment.cost_from_word(0, 0);
            if best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost) {
                best = Some((cost, alignment));
            }
            if cost == 0 {
                break;
            }
        }

        Ok(best
            .map(|(_, alignment)| alignment.deviations())
            .unwrap_or_default())
    }
}

// Secondary stress is ambiguous if there is a primary stress either before or after it.
fn is_secondary_next_to_primary(stresses: &[Stress], i: usize) -> bool {
    stresses[i] == Stress::Secondary
        && (stresses.get(i + 1) == Some(&Stress::Primary)
            || i > 0 && stresses.get(i - 1) == Some(&Stress::Primary))
}

/// Whether a word's syllable may stand in a meter position expecting `expected`.
fn syllable_stress_matches_meter(
    word_stresses: &[Stress],
    syllable_index: usize,
    expected: SyllableStress,
) -> bool {
    // Assumption: single-syllable words are inherently ambiguous and match any meter position.
    // An Either position matches any stress.
    if word_stresses.len() == 1 || expected == SyllableStress::Either {
        return true;
    }

    match word_stresses[syllable_index] {
        // Assumption: primary stress must match a stressed position
        Stress::Primary => expected == SyllableStress::Stressed,
        // Assumption: secondary stress adjacent to primary is ambiguous — matches any meter position
        Stress::Secondary if is_secondary_next_to_primary(word_stresses, syllable_index) => true,
        // Assumption: non-ambiguous secondary stress must match a stressed position
        Stress::Secondary => expected == SyllableStress::Stressed,
        // Assumption: unstressed syllables must match unstressed positions
        Stress::Unstressed => expected == SyllableStress::Unstressed,
    }
}

/// How a line fits its meter, judged by the line's least-deviating reading.
#[derive(Debug, PartialEq)]
pub enum MeterMatchResult {
    Match,
    /// Every deviation fit within the exception allowance.
    MatchWithExceptions {
        exceptions: Vec<MeterDeviation>,
    },
    /// `failures` are the deviations beyond the allowance. `exceptions` are
    /// the ones it covered, which come earlier in the line.
    NoMatch {
        failures: Vec<MeterDeviation>,
        exceptions: Vec<MeterDeviation>,
    },
}

/// One syllable-level departure of a line from its meter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeterDeviation {
    /// A syllable whose stress doesn't fit its position in the meter.
    StressMismatch {
        word_index: usize,
        syllable_index: usize,
        expected: SyllableStress,
        actual: Stress,
    },
    /// A syllable that fills no position in the meter: an extra syllable
    /// within the line, or one past the meter's end.
    ExtraSyllable {
        word_index: usize,
        syllable_index: usize,
        actual: Stress,
    },
    /// A position in the meter that no syllable fills, between words or after
    /// the line's last word.
    MissingSyllable {
        /// The word the gap comes before, or the line's word count when the
        /// gap follows its last word.
        before_word_index: usize,
        meter_syllable_index: usize,
        expected: SyllableStress,
    },
}

/// The number of syllable-level meter exceptions allowed in each fixed group of lines.
/// The default allows none, so every deviation is a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterExceptionConfig {
    allowed_exceptions: usize,
    lines_per_group: usize,
}

impl Default for MeterExceptionConfig {
    fn default() -> Self {
        Self {
            allowed_exceptions: 0,
            lines_per_group: 1,
        }
    }
}

impl MeterExceptionConfig {
    pub fn new(allowed_exceptions: usize, lines_per_group: usize) -> Option<Self> {
        (lines_per_group > 0).then_some(Self {
            allowed_exceptions,
            lines_per_group,
        })
    }

    pub fn allowed_exceptions(self) -> usize {
        self.allowed_exceptions
    }

    pub fn lines_per_group(self) -> usize {
        self.lines_per_group
    }
}

impl MeterMatchResult {
    /// Lets the allowance cover a line's first `allowed_exceptions` deviations;
    /// the rest are failures.
    pub(crate) fn from_deviations(
        mut deviations: Vec<MeterDeviation>,
        allowed_exceptions: usize,
    ) -> Self {
        let failures = deviations.split_off(allowed_exceptions.min(deviations.len()));
        let exceptions = deviations;

        if !failures.is_empty() {
            Self::NoMatch {
                failures,
                exceptions,
            }
        } else if exceptions.is_empty() {
            Self::Match
        } else {
            Self::MatchWithExceptions { exceptions }
        }
    }
}

/// A word's distinct stress patterns, in dictionary order.
fn distinct_stress_patterns(word: &WordEntry) -> Result<Vec<&[Stress]>, MeterCheckError> {
    let WordData::Known { pronunciations, .. } = &word.data else {
        return Err(MeterCheckError::UnknownWord(UnknownWordError {
            word: word.word.clone(),
        }));
    };

    let mut patterns: Vec<&[Stress]> = Vec::new();
    for pronunciation in pronunciations {
        let pattern = pronunciation.stress_pattern.as_slice();
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }
    Ok(patterns)
}

/// Finds the alignment of a line's syllables with one meter path that has the
/// fewest deviations. A syllable either fills the next meter position, deviating
/// if its stress doesn't fit there, or is extra; and a meter position can go
/// unfilled between words. So a single extra or missing syllable anywhere in a
/// line is one deviation, rather than shifting every syllable after it. Among
/// equally good alignments, filling a position is preferred, which places an
/// overlong line's extra syllables at its end.
///
/// How the rest of a line aligns depends only on where the search is in the line
/// and in the meter, so the fewest deviations from each such position are
/// memoized; otherwise the search is exponential in the number of words with
/// several stress patterns. The deviations themselves are traced once, through
/// the memoized costs, after the search.
struct Alignment<'a> {
    /// Each word's distinct stress patterns.
    stress_patterns: &'a [Vec<&'a [Stress]>],
    meter: &'a [SyllableStress],
    /// Memoized costs from the beginning of a word, by word and then meter position.
    word_costs: Vec<Option<usize>>,
    /// Each word's stress patterns' first rows in `syllable_costs`: a pattern has
    /// one row per syllable.
    syllable_rows: Vec<Vec<usize>>,
    /// Memoized costs from a syllable, by row and then meter position.
    syllable_costs: Vec<Option<usize>>,
}

impl<'a> Alignment<'a> {
    fn new(stress_patterns: &'a [Vec<&'a [Stress]>], meter: &'a [SyllableStress]) -> Self {
        let mut row_count = 0;
        let syllable_rows = stress_patterns
            .iter()
            .map(|patterns| {
                patterns
                    .iter()
                    .map(|pattern| {
                        let first_row = row_count;
                        row_count += pattern.len();
                        first_row
                    })
                    .collect()
            })
            .collect();
        let positions = meter.len() + 1;
        Self {
            stress_patterns,
            meter,
            word_costs: vec![None; stress_patterns.len() * positions],
            syllable_rows,
            syllable_costs: vec![None; row_count * positions],
        }
    }

    fn word_cost_index(&self, word_index: usize, meter_index: usize) -> usize {
        word_index * (self.meter.len() + 1) + meter_index
    }

    fn syllable_cost_index(
        &self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
    ) -> usize {
        let row = self.syllable_rows[word_index][pattern_index] + syllable_index;
        row * (self.meter.len() + 1) + meter_index
    }

    /// The deviations of the best alignment of the whole line.
    fn deviations(mut self) -> Vec<MeterDeviation> {
        let mut deviations = Vec::new();
        self.trace_from_word(0, 0, &mut deviations);
        deviations
    }

    /// The fewest deviations aligning words `word_index..` with meter positions
    /// `meter_index..`, starting at the beginning of a word.
    fn cost_from_word(&mut self, word_index: usize, meter_index: usize) -> usize {
        if word_index == self.stress_patterns.len() {
            // Out of syllables: the rest of the meter goes unfilled.
            return self.meter.len() - meter_index;
        }
        let memo_index = self.word_cost_index(word_index, meter_index);
        if let Some(cost) = self.word_costs[memo_index] {
            return cost;
        }

        let mut cost = usize::MAX;
        for pattern_index in 0..self.stress_patterns[word_index].len() {
            cost = cost.min(self.cost_from_syllable(word_index, pattern_index, 0, meter_index));
            if cost == 0 {
                break;
            }
        }
        // Leave this meter position unfilled before the word.
        if meter_index < self.meter.len() && cost > 0 {
            cost = cost.min(1 + self.cost_from_word(word_index, meter_index + 1));
        }

        self.word_costs[memo_index] = Some(cost);
        cost
    }

    /// The fewest deviations aligning the rest of the line, from syllable
    /// `syllable_index` of word `word_index` read with stress pattern
    /// `pattern_index`, with meter positions `meter_index..`.
    fn cost_from_syllable(
        &mut self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
    ) -> usize {
        let stresses = self.stress_patterns[word_index][pattern_index];
        if syllable_index == stresses.len() {
            return self.cost_from_word(word_index + 1, meter_index);
        }
        let memo_index =
            self.syllable_cost_index(word_index, pattern_index, syllable_index, meter_index);
        if let Some(cost) = self.syllable_costs[memo_index] {
            return cost;
        }

        let mut cost = usize::MAX;
        // The syllable fills the next meter position.
        if let Some(&expected) = self.meter.get(meter_index) {
            let fits = syllable_stress_matches_meter(stresses, syllable_index, expected);
            cost = usize::from(!fits)
                + self.cost_from_syllable(
                    word_index,
                    pattern_index,
                    syllable_index + 1,
                    meter_index + 1,
                );
        }
        // The syllable is extra.
        if cost > 0 {
            cost = cost.min(
                1 + self.cost_from_syllable(
                    word_index,
                    pattern_index,
                    syllable_index + 1,
                    meter_index,
                ),
            );
        }

        self.syllable_costs[memo_index] = Some(cost);
        cost
    }

    /// Appends the deviations of the best alignment from the beginning of word
    /// `word_index`, taking the first of equally good choices in the order
    /// [`Self::cost_from_word`] considers them.
    fn trace_from_word(
        &mut self,
        word_index: usize,
        meter_index: usize,
        deviations: &mut Vec<MeterDeviation>,
    ) {
        if word_index == self.stress_patterns.len() {
            deviations.extend((meter_index..self.meter.len()).map(|index| {
                MeterDeviation::MissingSyllable {
                    before_word_index: word_index,
                    meter_syllable_index: index,
                    expected: self.meter[index],
                }
            }));
            return;
        }

        let cost = self.cost_from_word(word_index, meter_index);
        for pattern_index in 0..self.stress_patterns[word_index].len() {
            if self.cost_from_syllable(word_index, pattern_index, 0, meter_index) == cost {
                return self.trace_from_syllable(
                    word_index,
                    pattern_index,
                    0,
                    meter_index,
                    deviations,
                );
            }
        }
        deviations.push(MeterDeviation::MissingSyllable {
            before_word_index: word_index,
            meter_syllable_index: meter_index,
            expected: self.meter[meter_index],
        });
        self.trace_from_word(word_index, meter_index + 1, deviations);
    }

    /// Appends the deviations of the best alignment from a syllable, taking the
    /// first of equally good choices in the order [`Self::cost_from_syllable`]
    /// considers them.
    fn trace_from_syllable(
        &mut self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
        deviations: &mut Vec<MeterDeviation>,
    ) {
        let stresses = self.stress_patterns[word_index][pattern_index];
        let Some(&actual) = stresses.get(syllable_index) else {
            return self.trace_from_word(word_index + 1, meter_index, deviations);
        };

        let cost = self.cost_from_syllable(word_index, pattern_index, syllable_index, meter_index);
        if let Some(&expected) = self.meter.get(meter_index) {
            let fits = syllable_stress_matches_meter(stresses, syllable_index, expected);
            let rest = self.cost_from_syllable(
                word_index,
                pattern_index,
                syllable_index + 1,
                meter_index + 1,
            );
            if usize::from(!fits) + rest == cost {
                if !fits {
                    deviations.push(MeterDeviation::StressMismatch {
                        word_index,
                        syllable_index,
                        expected,
                        actual,
                    });
                }
                return self.trace_from_syllable(
                    word_index,
                    pattern_index,
                    syllable_index + 1,
                    meter_index + 1,
                    deviations,
                );
            }
        }
        deviations.push(MeterDeviation::ExtraSyllable {
            word_index,
            syllable_index,
            actual,
        });
        self.trace_from_syllable(
            word_index,
            pattern_index,
            syllable_index + 1,
            meter_index,
            deviations,
        );
    }
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
    /// Parses a repeating multiline scheme of exact syllable counts or inclusive ranges.
    pub fn from_syllable_counts(s: &str) -> Result<Self, ParseSyllableCountSchemeError> {
        let scheme = s
            .lines()
            .enumerate()
            .map(|(line, text)| {
                text.parse::<SyllableCountSpecification>()
                    .map(|specification| specification.0)
                    .map_err(|source| ParseSyllableCountSchemeError { line, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { scheme })
    }

    /// Returns the meter specification for the given line index, wrapping around the scheme if necessary.
    fn line_specification(&self, line_index: usize) -> &MeterSpecification {
        &self.scheme[line_index % self.scheme.len()]
    }

    pub fn check_line(
        &self,
        line: &Line,
        line_index: usize,
    ) -> Result<MeterMatchResult, MeterCheckError> {
        self.line_specification(line_index).validate_line(line)
    }

    pub fn deviations_for_line(
        &self,
        line: &Line,
        line_index: usize,
    ) -> Result<Vec<MeterDeviation>, MeterCheckError> {
        self.line_specification(line_index)
            .deviations_for_line(line)
    }
}

#[cfg(test)]
mod tests {
    use crate::{CmuDictionary, line::Line};
    use std::sync::OnceLock;

    use super::*;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
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
    ) -> Vec<MeterDeviation> {
        let Ok(MeterMatchResult::NoMatch { failures, .. }) = result else {
            panic!("expected NoMatch, got {result:?}");
        };
        failures
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

    // --- MeterMatchResult failures ---

    #[test]
    fn validate_line_reports_every_failed_syllable() {
        // HELLO is x/, so each HELLO's first syllable misses a stress.
        let line = Line::new("hello hello", dict());
        let spec: MeterSpecification = "////".parse().unwrap();

        assert_eq!(
            assert_meter_no_match(spec.validate_line(&line)),
            [0, 1].map(|word_index| MeterDeviation::StressMismatch {
                word_index,
                syllable_index: 0,
                expected: S,
                actual: Stress::Unstressed,
            })
        );
    }

    #[test]
    fn validate_line_reports_a_word_past_the_end_of_the_meter() {
        // HELLO uses up the meter, leaving nothing for WORLD.
        let line = Line::new("hello world", dict());
        let spec: MeterSpecification = "x/".parse().unwrap();

        assert_eq!(
            assert_meter_no_match(spec.validate_line(&line)),
            [MeterDeviation::ExtraSyllable {
                word_index: 1,
                syllable_index: 0,
                actual: Stress::Primary,
            }]
        );
    }

    #[test]
    fn validate_line_reports_meter_left_over_after_the_last_word() {
        // Of the meter's two readings, the shorter one leaves less unfilled.
        let line = Line::new("", dict());
        let spec: MeterSpecification = "(x)/".parse().unwrap();

        assert_eq!(
            assert_meter_no_match(spec.validate_line(&line)),
            [MeterDeviation::MissingSyllable {
                before_word_index: 0,
                meter_syllable_index: 0,
                expected: S,
            }]
        );
    }

    #[test]
    fn alternate_pronunciation_can_match_meter() {
        // EVERY(1) is the two-syllable /x pronunciation; the base entry is /xx.
        let line = Line::new("every", dict());
        let spec: MeterSpecification = "/x".parse().unwrap();

        assert_meter_matches(spec.validate_line(&line));
    }

    #[test]
    fn less_promising_pronunciation_failure_is_not_reported() {
        let line = Line::new("and carries every wandering dream to light", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        let failures = assert_meter_no_match(spec.validate_line(&line));
        let failed_words: Vec<usize> = failures
            .iter()
            .map(|failure| match failure {
                MeterDeviation::StressMismatch { word_index, .. }
                | MeterDeviation::ExtraSyllable { word_index, .. } => *word_index,
                MeterDeviation::MissingSyllable { .. } => usize::MAX,
            })
            .collect();

        // EVERY's three-syllable /xx doesn't fit, but its two-syllable /x does,
        // so the best reading's problems lie elsewhere.
        assert!(failed_words.contains(&3), "{failures:?}");
        assert!(!failed_words.contains(&2), "{failures:?}");
    }

    #[test]
    fn deviation_analysis_continues_after_a_stress_mismatch() {
        // HELLO is x/ and KARAOKE is /x/x. Only HELLO's first syllable
        // deviates from this meter; analysis must continue through KARAOKE.
        let line = Line::new("hello karaoke", dict());
        let spec: MeterSpecification = "// /x/x".parse().unwrap();

        assert_eq!(
            spec.deviations_for_line(&line).unwrap(),
            vec![MeterDeviation::StressMismatch {
                word_index: 0,
                syllable_index: 0,
                expected: S,
                actual: Stress::Unstressed,
            }]
        );
    }

    #[test]
    fn deviation_analysis_counts_each_extra_and_missing_syllable() {
        let line = Line::new("hello", dict());
        let short_spec: MeterSpecification = "x".parse().unwrap();
        let long_spec: MeterSpecification = "x///".parse().unwrap();

        // Only HELLO's first syllable fits the meter.
        assert_eq!(
            short_spec.deviations_for_line(&line).unwrap(),
            vec![MeterDeviation::ExtraSyllable {
                word_index: 0,
                syllable_index: 1,
                actual: Stress::Primary,
            }]
        );
        assert_eq!(
            long_spec.deviations_for_line(&line).unwrap(),
            vec![
                MeterDeviation::MissingSyllable {
                    before_word_index: 1,
                    meter_syllable_index: 2,
                    expected: S,
                },
                MeterDeviation::MissingSyllable {
                    before_word_index: 1,
                    meter_syllable_index: 3,
                    expected: S,
                },
            ]
        );
    }

    #[test]
    fn deviation_analysis_chooses_the_pronunciation_with_fewest_exceptions() {
        let line = Line::new("every", dict());
        let spec: MeterSpecification = "/x".parse().unwrap();

        assert!(spec.deviations_for_line(&line).unwrap().is_empty());
    }

    #[test]
    fn deviation_analysis_counts_an_extra_syllable_within_the_line_once() {
        // BEAUTIFUL (/xx) has one more syllable than SUMMER (/x) in the
        // otherwise regular "a lover and a summer evening light"; the rest of
        // the line keeps its place in the meter.
        let line = Line::new("a lover and a beautiful evening light", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        assert_eq!(
            spec.deviations_for_line(&line).unwrap(),
            [MeterDeviation::ExtraSyllable {
                word_index: 4,
                syllable_index: 2,
                actual: Stress::Unstressed,
            }]
        );
    }

    #[test]
    fn deviation_analysis_counts_a_missing_syllable_between_words_once() {
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        // The line without its second "a": the gap falls before SUMMER.
        let dropped = Line::new("a lover and summer evening light", dict());
        assert_eq!(
            spec.deviations_for_line(&dropped).unwrap(),
            [MeterDeviation::MissingSyllable {
                before_word_index: 3,
                meter_syllable_index: 4,
                expected: U,
            }]
        );

        // A headless line, without its first "a".
        let headless = Line::new("lover and a summer evening light", dict());
        assert_eq!(
            spec.deviations_for_line(&headless).unwrap(),
            [MeterDeviation::MissingSyllable {
                before_word_index: 0,
                meter_syllable_index: 0,
                expected: U,
            }]
        );
    }

    #[test]
    fn deviation_analysis_counts_each_syllable_of_an_inverted_foot() {
        // NEVER (/x) where the meter expects x/: two syllables deviate.
        let line = Line::new("never a lover dreams upon the town", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        assert_eq!(spec.deviations_for_line(&line).unwrap().len(), 2);
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
    fn meter_scheme_check_line_uses_wrapped_line_index() {
        let ms: MeterScheme = "x/\n/x".parse().unwrap();
        let line = Line::new("hello", dict());

        assert_meter_matches(ms.check_line(&line, 0));
        let _ = assert_meter_no_match(ms.check_line(&line, 1));
        assert_meter_matches(ms.check_line(&line, 2));
    }

    #[test]
    fn syllable_count_scheme_supports_exact_counts_ranges_and_wrapping() {
        let scheme = MeterScheme::from_syllable_counts("2\n2-3\n1").unwrap();

        assert_meter_matches(scheme.check_line(&Line::new("cat dog", dict()), 0));
        assert_meter_matches(scheme.check_line(&Line::new("cat dog bird", dict()), 1));
        assert_meter_matches(scheme.check_line(&Line::new("cat", dict()), 2));
        assert_meter_matches(scheme.check_line(&Line::new("cat dog", dict()), 3));
        let _ = assert_meter_no_match(scheme.check_line(&Line::new("cat", dict()), 0));
    }

    #[test]
    fn syllable_count_scheme_reports_the_invalid_line() {
        let error = MeterScheme::from_syllable_counts("5\n9-7\n5").unwrap_err();

        assert_eq!(error.line, 1);
        assert!(matches!(
            error.source,
            ParseSyllableCountError::InvalidRange
        ));
        assert_eq!(error.to_string(), "line 2: min must be <= max");
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
