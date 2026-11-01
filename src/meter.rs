use std::collections::HashSet;

use crate::{
    Stress,
    error::{
        MeterCheckError, ParseMeterError, ParseMeterSchemeError, ParseSyllableCountError,
        ParseSyllableCountSchemeError, UnknownWordError,
    },
    function_words::is_function_word,
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
        Ok(MeterMatchResult::from_scansion(self.scan_line(line)?, 0))
    }

    /// Scans the line: finds its least-deviating reading, a choice of
    /// pronunciations, optional-meter path, and alignment of syllables with the
    /// meter, and records the role each syllable plays in it. The analysis
    /// continues after a mismatch, so every wrong stress, extra syllable, and
    /// missing syllable is recorded. A scansion without deviations means the
    /// line fits. Of equally deviating readings, the one promoting the fewest
    /// syllables wins.
    pub fn scan_line(&self, line: &Line) -> Result<Scansion, MeterCheckError> {
        let stress_patterns = line
            .words
            .iter()
            .map(distinct_stress_patterns)
            .collect::<Result<Vec<_>, _>>()?;
        let mut meters: Vec<&Meter> = self.possible_meters.iter().collect();
        meters.sort_by(|a, b| a.meter.cmp(&b.meter));

        // The first meter path with the lowest cost.
        let mut best: Option<(Cost, Alignment<'_>)> = None;
        for meter in meters {
            let mut alignment = Alignment::new(&stress_patterns, &meter.meter);
            let cost = alignment.cost_from_word(0, 0, PreviousSyllable::Unstressed);
            if best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost) {
                best = Some((cost, alignment));
            }
            if cost == Cost::ZERO {
                break;
            }
        }

        let (_, alignment) = best.expect("a meter specification has at least one meter path");
        Ok(alignment.scansion())
    }
}

// Secondary stress is ambiguous if there is a primary stress either before or after it.
fn is_secondary_next_to_primary(stresses: &[Stress], i: usize) -> bool {
    stresses[i] == Stress::Secondary
        && (stresses.get(i + 1) == Some(&Stress::Primary)
            || i > 0 && stresses.get(i - 1) == Some(&Stress::Primary))
}

fn is_stressed(stress: Stress) -> bool {
    stress != Stress::Unstressed
}

/// Whether a syllable's stress is ambiguous, so that it fits any meter
/// position.
fn is_ambiguous(word_stresses: &[Stress], syllable_index: usize) -> bool {
    // Assumption: single-syllable words are inherently ambiguous and match any meter position.
    // Assumption: secondary stress adjacent to primary is ambiguous — matches any meter position
    word_stresses.len() == 1 || is_secondary_next_to_primary(word_stresses, syllable_index)
}

/// How a word's syllable can stand in a meter position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyllableFit {
    Fits,
    /// An unstressed syllable on a beat after an unstressed syllable:
    /// promoted if the next syllable is unstressed or the line ends, and
    /// otherwise a mismatch.
    PromotedIfNextUnstressed,
    Mismatch,
}

impl SyllableFit {
    /// What filling the position costs, not counting a pending promotion,
    /// and what the next syllable sees before it.
    fn outcome(self, stress: Stress) -> (Cost, PreviousSyllable) {
        match self {
            Self::Fits => (Cost::ZERO, PreviousSyllable::of(stress)),
            Self::PromotedIfNextUnstressed => (Cost::ZERO, PreviousSyllable::PendingPromotion),
            Self::Mismatch => (Cost::DEVIATION, PreviousSyllable::of(stress)),
        }
    }
}

/// How a word's syllable can stand in a meter position expecting `expected`.
/// `previous_stressed` is whether the syllable before it in the line is
/// stressed.
fn syllable_fit(
    word_stresses: &[Stress],
    syllable_index: usize,
    expected: SyllableStress,
    previous_stressed: bool,
) -> SyllableFit {
    // An Either position matches any stress, and an ambiguous syllable any position.
    if expected == SyllableStress::Either || is_ambiguous(word_stresses, syllable_index) {
        return SyllableFit::Fits;
    }

    let fits = match word_stresses[syllable_index] {
        // Assumption: primary stress must match a stressed position
        Stress::Primary => expected == SyllableStress::Stressed,
        // Assumption: non-ambiguous secondary stress must match a stressed position
        Stress::Secondary => expected == SyllableStress::Stressed,
        Stress::Unstressed if expected == SyllableStress::Unstressed => true,
        // Promotion: an unstressed syllable may fill a stressed position when the
        // syllables on both sides of it are unstressed. The line's edges count
        // as unstressed. The syllable after it settles the promotion.
        Stress::Unstressed if previous_stressed => false,
        Stress::Unstressed => return SyllableFit::PromotedIfNextUnstressed,
    };
    if fits {
        SyllableFit::Fits
    } else {
        SyllableFit::Mismatch
    }
}

/// How a line fits its meter, judged by its scansion. `failures` and
/// `exceptions` split the scansion's deviations by the exception allowance.
#[derive(Debug, PartialEq)]
pub enum MeterMatchResult {
    Match {
        scansion: Scansion,
    },
    /// Every deviation fit within the exception allowance.
    MatchWithExceptions {
        exceptions: Vec<MeterDeviation>,
        scansion: Scansion,
    },
    /// `failures` are the deviations beyond the allowance. `exceptions` are
    /// the ones it covered, which come earlier in the line.
    NoMatch {
        failures: Vec<MeterDeviation>,
        exceptions: Vec<MeterDeviation>,
        scansion: Scansion,
    },
}

/// A line's scansion: the role each syllable plays in the line's
/// least-deviating reading, and the meter positions the reading leaves
/// unfilled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scansion {
    /// One for each of the line's words.
    pub words: Vec<WordScansion>,
    /// Meter positions left unfilled after the last word.
    pub trailing_gaps: Vec<Gap>,
}

/// A word's part of a scansion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordScansion {
    /// Meter positions left unfilled before the word.
    pub gaps_before: Vec<Gap>,
    /// The word's syllables, in the pronunciation the reading chose.
    pub syllables: Vec<SyllableScansion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyllableScansion {
    /// How the reading stresses the syllable.
    pub stress: ScannedStress,
    /// The meter position the syllable fills, or `None` for an extra syllable.
    pub position: Option<MeterPosition>,
}

/// How a scansion stresses a syllable. A syllable that fills a stressed or
/// unstressed position and fits it, including by promotion, takes that
/// position's stress. Any other syllable keeps its own stress: one that
/// mismatches its position, fills an `Either` position, or is extra. There,
/// a syllable whose own stress is ambiguous, like a one-syllable word's, is
/// `Ambiguous`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScannedStress {
    Stressed,
    Unstressed,
    Ambiguous,
}

/// A meter position that a syllable fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterPosition {
    pub index: usize,
    /// The stress the meter expects there.
    pub expected: SyllableStress,
    /// How the syllable's stress fits the position.
    pub fit: Fit,
}

/// How a syllable's stress fits the meter position it fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fit {
    Fits,
    /// An unstressed syllable on a beat, between unstressed syllables in its
    /// own word or a neighboring one (Derek Attridge's promotion). A line's
    /// edges count as unstressed, as in the last syllable of "thou art more
    /// lovely and more TEM-per-ATE".
    Promoted,
    Mismatch,
}

/// A meter position that no syllable fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub index: usize,
    /// The stress the meter expects there.
    pub expected: SyllableStress,
}

impl Scansion {
    /// Where the scansion departs from the meter, in line order.
    pub fn deviations(&self) -> impl Iterator<Item = MeterDeviation> + '_ {
        let word_count = self.words.len();
        self.words
            .iter()
            .enumerate()
            .flat_map(|(word_index, word)| {
                let gaps = word
                    .gaps_before
                    .iter()
                    .map(move |gap| gap.deviation(word_index));
                let syllables = word.syllables.iter().enumerate().filter_map(
                    move |(syllable_index, syllable)| {
                        syllable.deviation(word_index, syllable_index)
                    },
                );
                gaps.chain(syllables)
            })
            .chain(
                self.trailing_gaps
                    .iter()
                    .map(move |gap| gap.deviation(word_count)),
            )
    }
}

impl SyllableScansion {
    /// The syllable's deviation from the meter, if it mismatches its position
    /// or is extra.
    fn deviation(&self, word_index: usize, syllable_index: usize) -> Option<MeterDeviation> {
        match self.position {
            None => Some(MeterDeviation::ExtraSyllable {
                word_index,
                syllable_index,
                actual: self.stress,
            }),
            Some(MeterPosition {
                expected,
                fit: Fit::Mismatch,
                ..
            }) => Some(MeterDeviation::StressMismatch {
                word_index,
                syllable_index,
                expected,
                actual: self.stress,
            }),
            Some(_) => None,
        }
    }
}

impl ScannedStress {
    /// A syllable's own stress, for where no meter position decides it.
    fn own(stress: Stress, ambiguous: bool) -> Self {
        if ambiguous {
            Self::Ambiguous
        } else if is_stressed(stress) {
            Self::Stressed
        } else {
            Self::Unstressed
        }
    }
}

impl MeterPosition {
    /// How a scansion stresses a syllable that fills the position, given the
    /// syllable's own stress.
    fn scanned_stress(&self, own: ScannedStress) -> ScannedStress {
        match (self.expected, self.fit) {
            (_, Fit::Mismatch) | (SyllableStress::Either, _) => own,
            (SyllableStress::Stressed, _) => ScannedStress::Stressed,
            (SyllableStress::Unstressed, _) => ScannedStress::Unstressed,
        }
    }
}

impl Gap {
    /// The gap as a deviation, given the word it comes before.
    fn deviation(&self, before_word_index: usize) -> MeterDeviation {
        MeterDeviation::MissingSyllable {
            before_word_index,
            meter_syllable_index: self.index,
            expected: self.expected,
        }
    }
}

/// One syllable-level departure of a line from its meter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeterDeviation {
    /// A syllable whose stress doesn't fit its position in the meter.
    StressMismatch {
        word_index: usize,
        syllable_index: usize,
        expected: SyllableStress,
        /// How the scansion stresses the syllable.
        actual: ScannedStress,
    },
    /// A syllable that fills no position in the meter: an extra syllable
    /// within the line, or one past the meter's end.
    ExtraSyllable {
        word_index: usize,
        syllable_index: usize,
        /// How the scansion stresses the syllable.
        actual: ScannedStress,
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
    pub(crate) fn from_scansion(scansion: Scansion, allowed_exceptions: usize) -> Self {
        let mut deviations: Vec<MeterDeviation> = scansion.deviations().collect();
        let failures = deviations.split_off(allowed_exceptions.min(deviations.len()));
        let exceptions = deviations;

        if !failures.is_empty() {
            Self::NoMatch {
                failures,
                exceptions,
                scansion,
            }
        } else if exceptions.is_empty() {
            Self::Match { scansion }
        } else {
            Self::MatchWithExceptions {
                exceptions,
                scansion,
            }
        }
    }
}

const UNSTRESSED_MONOSYLLABLE: &[Stress] = &[Stress::Unstressed];

/// A word's distinct stress patterns, in dictionary order, then an unstressed
/// one for a function word with a one-syllable pronunciation. A one-syllable
/// word fits any meter position, so the unstressed pattern only lets a
/// neighbor be promoted, as in "as BEAU-ti-FUL as day".
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
    if is_function_word(&word.word)
        && patterns.iter().any(|pattern| pattern.len() == 1)
        && !patterns.contains(&UNSTRESSED_MONOSYLLABLE)
    {
        patterns.push(UNSTRESSED_MONOSYLLABLE);
    }
    Ok(patterns)
}

/// A reading's deviations, then its promotions. Readings compare by
/// deviations first, and of equally deviating readings, the one promoting
/// fewer syllables is the more natural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    deviations: usize,
    promotions: usize,
}

impl Cost {
    const ZERO: Self = Self::new(0, 0);
    const DEVIATION: Self = Self::new(1, 0);
    const PROMOTION: Self = Self::new(0, 1);
    const MAX: Self = Self::new(usize::MAX, usize::MAX);

    const fn new(deviations: usize, promotions: usize) -> Self {
        Self {
            deviations,
            promotions,
        }
    }
}

impl std::ops::Add for Cost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(
            self.deviations.saturating_add(other.deviations),
            self.promotions.saturating_add(other.promotions),
        )
    }
}

/// What the alignment knows about the syllable before the one it's placing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviousSyllable {
    /// An unstressed syllable, or the start of the line.
    Unstressed,
    Stressed,
    /// An unstressed syllable on a beat after an unstressed syllable:
    /// promoted if the syllable after it is unstressed too or the line ends,
    /// and otherwise a mismatch.
    PendingPromotion,
}

impl PreviousSyllable {
    const COUNT: usize = 3;

    fn of(stress: Stress) -> Self {
        if is_stressed(stress) {
            Self::Stressed
        } else {
            Self::Unstressed
        }
    }

    fn is_stressed(self) -> bool {
        self == Self::Stressed
    }

    /// What a pending promotion costs once the syllable after it is known.
    /// `None` is the end of the line, which counts as unstressed.
    fn resolution_cost(self, next: Option<Stress>) -> Cost {
        match self {
            Self::PendingPromotion if next.is_some_and(is_stressed) => Cost::DEVIATION,
            Self::PendingPromotion => Cost::PROMOTION,
            Self::Unstressed | Self::Stressed => Cost::ZERO,
        }
    }
}

/// What a line's best alignment does at one point in the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// A syllable fills the next meter position.
    Filled {
        word_index: usize,
        /// The syllable's stress in the pronunciation the reading chose, and
        /// whether it's ambiguous.
        stress: Stress,
        ambiguous: bool,
        meter_index: usize,
        expected: SyllableStress,
        fit: SyllableFit,
    },
    /// A syllable fills no meter position.
    Extra {
        word_index: usize,
        stress: Stress,
        ambiguous: bool,
    },
    /// A meter position that no syllable fills.
    Missing {
        before_word_index: usize,
        meter_index: usize,
        expected: SyllableStress,
    },
}

impl Step {
    /// The stress of the syllable this step places, if it places one.
    fn syllable_stress(&self) -> Option<Stress> {
        match *self {
            Self::Filled { stress, .. } | Self::Extra { stress, .. } => Some(stress),
            Self::Missing { .. } => None,
        }
    }
}

impl Scansion {
    /// The scansion of a line of `word_count` words, from its best
    /// alignment's steps.
    fn from_steps(steps: &[Step], word_count: usize) -> Self {
        let mut scansion = Self {
            words: vec![WordScansion::default(); word_count],
            trailing_gaps: Vec::new(),
        };
        for (index, step) in steps.iter().enumerate() {
            match *step {
                Step::Filled {
                    word_index,
                    stress,
                    ambiguous,
                    meter_index,
                    expected,
                    fit,
                } => {
                    let fit = match fit {
                        SyllableFit::Fits => Fit::Fits,
                        // The next syllable settles a promotion; the line's end
                        // counts as unstressed.
                        SyllableFit::PromotedIfNextUnstressed => {
                            let next = steps[index + 1..].iter().find_map(Step::syllable_stress);
                            if next.is_some_and(is_stressed) {
                                Fit::Mismatch
                            } else {
                                Fit::Promoted
                            }
                        }
                        SyllableFit::Mismatch => Fit::Mismatch,
                    };
                    let position = MeterPosition {
                        index: meter_index,
                        expected,
                        fit,
                    };
                    scansion.words[word_index].syllables.push(SyllableScansion {
                        stress: position.scanned_stress(ScannedStress::own(stress, ambiguous)),
                        position: Some(position),
                    });
                }
                Step::Extra {
                    word_index,
                    stress,
                    ambiguous,
                } => scansion.words[word_index].syllables.push(SyllableScansion {
                    stress: ScannedStress::own(stress, ambiguous),
                    position: None,
                }),
                Step::Missing {
                    before_word_index,
                    meter_index,
                    expected,
                } => {
                    let gap = Gap {
                        index: meter_index,
                        expected,
                    };
                    match scansion.words.get_mut(before_word_index) {
                        Some(word) => word.gaps_before.push(gap),
                        None => scansion.trailing_gaps.push(gap),
                    }
                }
            }
        }
        scansion
    }
}

/// Finds the lowest-cost alignment of a line's syllables with one meter path.
/// A syllable either fills the next meter position, deviating if its stress
/// doesn't fit there, or is extra; and a meter position can go unfilled
/// between words. So a single extra or missing syllable anywhere in a line is
/// one deviation, rather than shifting every syllable after it. Among equally
/// good alignments, filling a position is preferred, which places an overlong
/// line's extra syllables at its end.
///
/// Whether an unstressed syllable can be promoted to a beat depends on the
/// syllables on both sides of it, which may be in other words. The search
/// checks the syllable before when it places one, and leaves the promotion
/// pending for the syllable after to settle. So it carries the previous
/// syllable, pending promotion and all, from syllable to syllable and word to
/// word.
///
/// How the rest of a line aligns depends only on where the search is in the
/// line and in the meter, and on the previous syllable, so the lowest cost
/// from each such position is memoized; otherwise the search is exponential in
/// the number of words with several stress patterns. After the search, the
/// best alignment's steps are traced once, through the memoized costs, and
/// the scansion is built from them.
struct Alignment<'a> {
    /// Each word's distinct stress patterns.
    stress_patterns: &'a [Vec<&'a [Stress]>],
    meter: &'a [SyllableStress],
    /// Memoized costs from the beginning of a word, by word, meter position,
    /// and previous syllable.
    word_costs: Vec<Option<Cost>>,
    /// Each word's stress patterns' first rows in `syllable_costs`: a pattern has
    /// one row per syllable.
    syllable_rows: Vec<Vec<usize>>,
    /// Memoized costs from a syllable, by row, meter position, and previous
    /// syllable.
    syllable_costs: Vec<Option<Cost>>,
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
            word_costs: vec![None; stress_patterns.len() * positions * PreviousSyllable::COUNT],
            syllable_rows,
            syllable_costs: vec![None; row_count * positions * PreviousSyllable::COUNT],
        }
    }

    fn word_cost_index(
        &self,
        word_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> usize {
        (word_index * (self.meter.len() + 1) + meter_index) * PreviousSyllable::COUNT
            + previous as usize
    }

    fn syllable_cost_index(
        &self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> usize {
        let row = self.syllable_rows[word_index][pattern_index] + syllable_index;
        (row * (self.meter.len() + 1) + meter_index) * PreviousSyllable::COUNT + previous as usize
    }

    /// The scansion of the line's best alignment.
    fn scansion(self) -> Scansion {
        let word_count = self.stress_patterns.len();
        Scansion::from_steps(&self.steps(), word_count)
    }

    /// The lowest cost aligning words `word_index..` with meter positions
    /// `meter_index..`, starting at the beginning of a word.
    fn cost_from_word(
        &mut self,
        word_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> Cost {
        if word_index == self.stress_patterns.len() {
            // Out of syllables: the rest of the meter goes unfilled.
            return Cost::new(self.meter.len() - meter_index, 0) + previous.resolution_cost(None);
        }
        let memo_index = self.word_cost_index(word_index, meter_index, previous);
        if let Some(cost) = self.word_costs[memo_index] {
            return cost;
        }

        let mut cost = Cost::MAX;
        for pattern_index in 0..self.stress_patterns[word_index].len() {
            cost = cost.min(self.cost_from_syllable(
                word_index,
                pattern_index,
                0,
                meter_index,
                previous,
            ));
            if cost == Cost::ZERO {
                break;
            }
        }
        // Leave this meter position unfilled before the word.
        if meter_index < self.meter.len() && cost.deviations > 0 {
            cost = cost
                .min(Cost::DEVIATION + self.cost_from_word(word_index, meter_index + 1, previous));
        }

        self.word_costs[memo_index] = Some(cost);
        cost
    }

    /// The lowest cost aligning the rest of the line, from syllable
    /// `syllable_index` of word `word_index` read with stress pattern
    /// `pattern_index`, with meter positions `meter_index..`, including
    /// settling a promotion pending on the syllable before.
    fn cost_from_syllable(
        &mut self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> Cost {
        let stresses = self.stress_patterns[word_index][pattern_index];
        let Some(&stress) = stresses.get(syllable_index) else {
            return self.cost_from_word(word_index + 1, meter_index, previous);
        };
        let memo_index = self.syllable_cost_index(
            word_index,
            pattern_index,
            syllable_index,
            meter_index,
            previous,
        );
        if let Some(cost) = self.syllable_costs[memo_index] {
            return cost;
        }

        let mut cost = Cost::MAX;
        // The syllable fills the next meter position.
        if let Some(&expected) = self.meter.get(meter_index) {
            let fit = syllable_fit(stresses, syllable_index, expected, previous.is_stressed());
            let (fit_cost, next_previous) = fit.outcome(stress);
            cost = fit_cost
                + self.cost_from_syllable(
                    word_index,
                    pattern_index,
                    syllable_index + 1,
                    meter_index + 1,
                    next_previous,
                );
        }
        // The syllable is extra.
        if cost.deviations > 0 {
            cost = cost.min(
                Cost::DEVIATION
                    + self.cost_from_syllable(
                        word_index,
                        pattern_index,
                        syllable_index + 1,
                        meter_index,
                        PreviousSyllable::of(stress),
                    ),
            );
        }
        // This syllable settles a promotion pending on the one before.
        let cost = previous.resolution_cost(Some(stress)) + cost;

        self.syllable_costs[memo_index] = Some(cost);
        cost
    }

    /// Where the best alignment from the beginning of word `word_index` starts
    /// filling the meter, after any positions it leaves unfilled, and the
    /// stress pattern it reads the word with: the first of equally good
    /// choices in the order [`Self::cost_from_word`] considers them. `None`
    /// past the last word.
    fn word_start(
        &mut self,
        word_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> Option<(usize, usize)> {
        if word_index == self.stress_patterns.len() {
            return None;
        }
        let pattern_count = self.stress_patterns[word_index].len();
        let mut meter_index = meter_index;
        loop {
            let cost = self.cost_from_word(word_index, meter_index, previous);
            if let Some(pattern_index) = (0..pattern_count).find(|&pattern_index| {
                self.cost_from_syllable(word_index, pattern_index, 0, meter_index, previous) == cost
            }) {
                return Some((meter_index, pattern_index));
            }
            meter_index += 1;
        }
    }

    /// The steps of the best alignment of the whole line, in line order.
    fn steps(mut self) -> Vec<Step> {
        // Every meter position is filled or missing, and syllables may be extra.
        let mut steps = Vec::with_capacity(self.meter.len());
        let (mut word_index, mut meter_index) = (0, 0);
        let mut previous = PreviousSyllable::Unstressed;
        loop {
            let start = self.word_start(word_index, meter_index, previous);
            let filled_from = start.map_or(self.meter.len(), |(start, _)| start);
            steps.extend((meter_index..filled_from).map(|meter_index| Step::Missing {
                before_word_index: word_index,
                meter_index,
                expected: self.meter[meter_index],
            }));
            let Some((start, pattern_index)) = start else {
                return steps;
            };
            meter_index = start;

            let stresses = self.stress_patterns[word_index][pattern_index];
            for (syllable_index, &stress) in stresses.iter().enumerate() {
                let ambiguous = is_ambiguous(stresses, syllable_index);
                match self.best_fill(
                    word_index,
                    pattern_index,
                    syllable_index,
                    meter_index,
                    previous,
                ) {
                    Some((expected, fit)) => {
                        steps.push(Step::Filled {
                            word_index,
                            stress,
                            ambiguous,
                            meter_index,
                            expected,
                            fit,
                        });
                        meter_index += 1;
                        previous = fit.outcome(stress).1;
                    }
                    None => {
                        steps.push(Step::Extra {
                            word_index,
                            stress,
                            ambiguous,
                        });
                        previous = PreviousSyllable::of(stress);
                    }
                }
            }
            word_index += 1;
        }
    }

    /// The expected stress and the fit, if the best alignment from syllable
    /// `syllable_index` of word `word_index`, read with stress pattern
    /// `pattern_index`, has it fill meter position `meter_index`; `None` if
    /// the syllable is extra. Filling wins ties, as in
    /// [`Self::cost_from_syllable`].
    fn best_fill(
        &mut self,
        word_index: usize,
        pattern_index: usize,
        syllable_index: usize,
        meter_index: usize,
        previous: PreviousSyllable,
    ) -> Option<(SyllableStress, SyllableFit)> {
        let stresses = self.stress_patterns[word_index][pattern_index];
        let stress = stresses[syllable_index];
        let &expected = self.meter.get(meter_index)?;
        let fit = syllable_fit(stresses, syllable_index, expected, previous.is_stressed());
        let (fit_cost, next_previous) = fit.outcome(stress);
        let filled = previous.resolution_cost(Some(stress))
            + fit_cost
            + self.cost_from_syllable(
                word_index,
                pattern_index,
                syllable_index + 1,
                meter_index + 1,
                next_previous,
            );
        let best = self.cost_from_syllable(
            word_index,
            pattern_index,
            syllable_index,
            meter_index,
            previous,
        );
        (filled == best).then_some((expected, fit))
    }
}

/// The largest syllable count a syllable-count specification accepts. Each count
/// in a range becomes its own meter path, so an unbounded count could stall
/// parsing and analysis; no line of verse comes near this.
pub const MAX_SYLLABLE_COUNT: usize = 100;

#[derive(Debug)]
pub struct SyllableCountSpecification(MeterSpecification);

impl std::str::FromStr for SyllableCountSpecification {
    type Err = ParseSyllableCountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let count = |text: &str| {
            text.trim()
                .parse::<usize>()
                .map_err(|_| ParseSyllableCountError::InvalidNumber)
        };
        let (min, max) = match s.split_once('-') {
            Some((min, max)) => (count(min)?, count(max)?),
            None => {
                let n = count(s)?;
                (n, n)
            }
        };
        if min > max {
            return Err(ParseSyllableCountError::InvalidRange);
        }
        if max > MAX_SYLLABLE_COUNT {
            return Err(ParseSyllableCountError::TooLarge);
        }
        Ok(Self(MeterSpecification::from_syllable_range(min, max)))
    }
}

impl SyllableCountSpecification {
    pub fn validate_line(&self, line: &Line) -> Result<MeterMatchResult, MeterCheckError> {
        self.0.validate_line(line)
    }
}

/// One meter specification per line of the poem, repeating from the first
/// when the poem is longer. Blank lines in the scheme's text are layout, such
/// as space between stanzas, and don't count as scheme lines; parse errors
/// still number lines as they appear in the text.
#[derive(Debug)]
pub struct MeterScheme {
    scheme: Vec<MeterSpecification>,
}

/// The scheme text's lines that aren't blank, numbered by their place in the text.
fn scheme_lines(s: &str) -> impl Iterator<Item = (usize, &str)> {
    s.lines()
        .enumerate()
        .filter(|(_, text)| !text.trim().is_empty())
}

impl std::str::FromStr for MeterScheme {
    type Err = ParseMeterSchemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scheme = scheme_lines(s)
            .map(|(line, text)| {
                text.parse::<MeterSpecification>()
                    .map_err(|source| ParseMeterSchemeError::Line { line, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if scheme.is_empty() {
            return Err(ParseMeterSchemeError::Empty);
        }
        Ok(MeterScheme { scheme })
    }
}

impl MeterScheme {
    /// Parses a repeating multiline scheme of exact syllable counts or inclusive
    /// ranges, each at most [`MAX_SYLLABLE_COUNT`].
    pub fn from_syllable_counts(s: &str) -> Result<Self, ParseSyllableCountSchemeError> {
        let scheme = scheme_lines(s)
            .map(|(line, text)| {
                text.parse::<SyllableCountSpecification>()
                    .map(|specification| specification.0)
                    .map_err(|source| ParseSyllableCountSchemeError::Line { line, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if scheme.is_empty() {
            return Err(ParseSyllableCountSchemeError::Empty);
        }
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

    pub fn scan_line(&self, line: &Line, line_index: usize) -> Result<Scansion, MeterCheckError> {
        self.line_specification(line_index).scan_line(line)
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
        assert!(
            matches!(result, Ok(MeterMatchResult::Match { .. })),
            "{result:?}"
        );
    }

    fn deviations(spec: &MeterSpecification, line: &Line) -> Vec<MeterDeviation> {
        spec.scan_line(line).unwrap().deviations().collect()
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
                actual: ScannedStress::Unstressed,
            })
        );
    }

    #[test]
    fn validate_line_reports_a_word_past_the_end_of_the_meter() {
        // HELLO uses up the meter, leaving nothing for WORLD. No position
        // decides the one-syllable word's stress.
        let line = Line::new("hello world", dict());
        let spec: MeterSpecification = "x/".parse().unwrap();

        assert_eq!(
            assert_meter_no_match(spec.validate_line(&line)),
            [MeterDeviation::ExtraSyllable {
                word_index: 1,
                syllable_index: 0,
                actual: ScannedStress::Ambiguous,
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
            deviations(&spec, &line),
            vec![MeterDeviation::StressMismatch {
                word_index: 0,
                syllable_index: 0,
                expected: S,
                actual: ScannedStress::Unstressed,
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
            deviations(&short_spec, &line),
            vec![MeterDeviation::ExtraSyllable {
                word_index: 0,
                syllable_index: 1,
                actual: ScannedStress::Stressed,
            }]
        );
        assert_eq!(
            deviations(&long_spec, &line),
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

        assert!(deviations(&spec, &line).is_empty());
    }

    #[test]
    fn deviation_analysis_counts_an_extra_syllable_within_the_line_once() {
        // BEAUTIFUL (/xx) has one more syllable than SUMMER (/x) in the
        // otherwise regular "a lover and a summer evening light"; the rest of
        // the line keeps its place in the meter.
        let line = Line::new("a lover and a beautiful evening light", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        assert_eq!(
            deviations(&spec, &line),
            [MeterDeviation::ExtraSyllable {
                word_index: 4,
                syllable_index: 2,
                actual: ScannedStress::Unstressed,
            }]
        );
    }

    #[test]
    fn deviation_analysis_counts_a_missing_syllable_between_words_once() {
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        // The line without its second "a": the gap falls before SUMMER.
        let dropped = Line::new("a lover and summer evening light", dict());
        assert_eq!(
            deviations(&spec, &dropped),
            [MeterDeviation::MissingSyllable {
                before_word_index: 3,
                meter_syllable_index: 4,
                expected: U,
            }]
        );

        // A headless line, without its first "a".
        let headless = Line::new("lover and a summer evening light", dict());
        assert_eq!(
            deviations(&spec, &headless),
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

        assert_eq!(deviations(&spec, &line).len(), 2);
    }

    // --- promotion ---

    /// A promoted syllable's word and syllable indices.
    fn promoted(word_index: usize, syllable_index: usize) -> (usize, usize) {
        (word_index, syllable_index)
    }

    /// The scansion's promoted syllables, in line order.
    fn promotions(scansion: &Scansion) -> Vec<(usize, usize)> {
        let mut promotions = Vec::new();
        for (word_index, word) in scansion.words.iter().enumerate() {
            for (syllable_index, syllable) in word.syllables.iter().enumerate() {
                if matches!(
                    syllable.position,
                    Some(MeterPosition {
                        fit: Fit::Promoted,
                        ..
                    })
                ) {
                    promotions.push(promoted(word_index, syllable_index));
                }
            }
        }
        promotions
    }

    fn assert_meter_matches_with_promotions(
        result: Result<MeterMatchResult, MeterCheckError>,
        expected: &[(usize, usize)],
    ) {
        let Ok(MeterMatchResult::Match { scansion }) = result else {
            panic!("expected Match, got {result:?}");
        };
        assert_eq!(promotions(&scansion), expected);
    }

    fn unstressed_on_beat(word_index: usize, syllable_index: usize) -> MeterDeviation {
        MeterDeviation::StressMismatch {
            word_index,
            syllable_index,
            expected: S,
            actual: ScannedStress::Unstressed,
        }
    }

    #[test]
    fn promotes_an_unstressed_syllable_between_unstressed_syllables() {
        // ACCURACY is /xxx.
        let line = Line::new("accuracy", dict());
        let spec: MeterSpecification = "/x/x".parse().unwrap();

        assert_meter_matches_with_promotions(spec.validate_line(&line), &[promoted(0, 2)]);
    }

    #[test]
    fn promotes_a_syllable_at_the_end_of_the_line() {
        // TEMPERATE is /x or /xx. The line's end counts as unstressed.
        let line = Line::new("thou art more lovely and more temperate", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        assert_meter_matches_with_promotions(spec.validate_line(&line), &[promoted(6, 2)]);
    }

    #[test]
    fn does_not_promote_a_syllable_beside_a_stress_in_its_word() {
        // BEAUTIFUL is /xx, so its second syllable follows a stress.
        let line = Line::new("beautiful", dict());
        let spec: MeterSpecification = "//x".parse().unwrap();

        assert_eq!(
            assert_meter_no_match(spec.validate_line(&line)),
            [unstressed_on_beat(0, 1)]
        );
    }

    #[test]
    fn promotion_depends_on_the_first_syllable_of_the_next_word() {
        // ARRAY is x/, but DREAM is stressed.
        assert_meter_matches_with_promotions(
            "x/x/x/"
                .parse::<MeterSpecification>()
                .unwrap()
                .validate_line(&Line::new("a beautiful array", dict())),
            &[promoted(1, 2)],
        );
        assert_eq!(
            assert_meter_no_match(
                "x/x/x"
                    .parse::<MeterSpecification>()
                    .unwrap()
                    .validate_line(&Line::new("a beautiful dream", dict()))
            ),
            [unstressed_on_beat(1, 2)]
        );
    }

    #[test]
    fn promotion_depends_on_the_last_syllable_of_the_previous_word() {
        // ADRIENNE is xx/. THE can be unstressed, but SWEET is stressed.
        let spec: MeterSpecification = "x/x/".parse().unwrap();

        assert_meter_matches_with_promotions(
            spec.validate_line(&Line::new("the adrienne", dict())),
            &[promoted(1, 0)],
        );
        assert_eq!(
            assert_meter_no_match(spec.validate_line(&Line::new("sweet adrienne", dict()))),
            [unstressed_on_beat(1, 0)]
        );
    }

    #[test]
    fn function_words_can_go_unstressed_beside_a_promotion() {
        // CMUdict only stresses AS and MY.
        assert_meter_matches_with_promotions(
            "x/x/x/"
                .parse::<MeterSpecification>()
                .unwrap()
                .validate_line(&Line::new("As beautiful as day", dict())),
            &[promoted(1, 2)],
        );
        assert_meter_matches_with_promotions(
            "x/x/"
                .parse::<MeterSpecification>()
                .unwrap()
                .validate_line(&Line::new("my adrienne", dict())),
            &[promoted(1, 0)],
        );
    }

    #[test]
    fn promotion_reads_the_next_word_unstressed_when_it_can_be() {
        // IS can be stressed or unstressed; unstressed, it lets BEAUTIFUL's
        // last syllable take a beat.
        let line = Line::new("the beautiful is never what it seems", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        assert_meter_matches_with_promotions(spec.validate_line(&line), &[promoted(1, 2)]);
    }

    #[test]
    fn prefers_a_reading_without_promotions() {
        // GENUINELY's first pronunciation is /xxx, which fits only by
        // promoting its third syllable; its second, /x\x, fits outright.
        let line = Line::new("genuinely", dict());
        let spec: MeterSpecification = "/x/x".parse().unwrap();

        assert_meter_matches_with_promotions(spec.validate_line(&line), &[]);
    }

    #[test]
    fn a_failing_lines_scansion_marks_its_promotions() {
        let line = Line::new("the beautiful is never what it", dict());
        let spec: MeterSpecification = "x/x/x/x/x/".parse().unwrap();

        let Ok(MeterMatchResult::NoMatch {
            failures,
            exceptions,
            scansion,
        }) = spec.validate_line(&line)
        else {
            panic!("expected NoMatch");
        };
        assert_eq!(
            failures,
            [MeterDeviation::MissingSyllable {
                before_word_index: 6,
                meter_syllable_index: 9,
                expected: S,
            }]
        );
        assert!(exceptions.is_empty());
        assert_eq!(promotions(&scansion), [promoted(1, 2)]);
    }

    fn pending(word_index: usize, meter_index: usize) -> Step {
        Step::Filled {
            word_index,
            stress: Stress::Unstressed,
            ambiguous: false,
            meter_index,
            expected: S,
            fit: SyllableFit::PromotedIfNextUnstressed,
        }
    }

    #[test]
    fn the_next_syllable_after_a_gap_settles_a_promotion() {
        // A stressed one-syllable word blocks the promotion, though it scans
        // unstressed on its offbeat.
        let steps = [
            pending(0, 0),
            Step::Missing {
                before_word_index: 1,
                meter_index: 1,
                expected: U,
            },
            Step::Filled {
                word_index: 1,
                stress: Stress::Primary,
                ambiguous: true,
                meter_index: 2,
                expected: U,
                fit: SyllableFit::Fits,
            },
        ];

        assert_eq!(
            Scansion::from_steps(&steps, 2),
            Scansion {
                words: vec![
                    WordScansion {
                        gaps_before: vec![],
                        syllables: vec![SyllableScansion {
                            stress: ScannedStress::Unstressed,
                            position: Some(MeterPosition {
                                index: 0,
                                expected: S,
                                fit: Fit::Mismatch,
                            }),
                        }],
                    },
                    WordScansion {
                        gaps_before: vec![Gap {
                            index: 1,
                            expected: U,
                        }],
                        syllables: vec![SyllableScansion {
                            stress: ScannedStress::Unstressed,
                            position: Some(MeterPosition {
                                index: 2,
                                expected: U,
                                fit: Fit::Fits,
                            }),
                        }],
                    },
                ],
                trailing_gaps: vec![],
            }
        );
    }

    #[test]
    fn an_extra_syllable_or_the_line_end_can_settle_a_promotion() {
        let steps = [
            pending(0, 0),
            Step::Extra {
                word_index: 0,
                stress: Stress::Unstressed,
                ambiguous: false,
            },
            pending(1, 1),
        ];
        let scansion = Scansion::from_steps(&steps, 2);

        assert_eq!(promotions(&scansion), [promoted(0, 0), promoted(1, 0)]);
        // A promoted syllable takes its beat's stress.
        assert_eq!(
            scansion.words[0].syllables[0].stress,
            ScannedStress::Stressed
        );
    }

    // --- scansion ---

    fn scan(meter: &str, text: &str) -> Scansion {
        meter
            .parse::<MeterSpecification>()
            .unwrap()
            .scan_line(&Line::new(text, dict()))
            .unwrap()
    }

    /// The scansion's stresses, a word at a time: `/` stressed, `x`
    /// unstressed, `?` ambiguous.
    fn marks(scansion: &Scansion) -> String {
        scansion
            .words
            .iter()
            .map(|word| {
                word.syllables
                    .iter()
                    .map(|syllable| match syllable.stress {
                        ScannedStress::Stressed => '/',
                        ScannedStress::Unstressed => 'x',
                        ScannedStress::Ambiguous => '?',
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn syllables_take_the_stress_of_positions_they_fit() {
        // One-syllable words take their positions' stress, and so does
        // TEMPERATE's promoted last syllable.
        assert_eq!(
            marks(&scan(
                "x/x/x/x/x/",
                "thou art more lovely and more temperate"
            )),
            "x / x /x / x /x/"
        );
    }

    #[test]
    fn a_mismatched_syllable_keeps_its_own_stress() {
        // NEVER is /x where the meter expects x/.
        assert_eq!(
            marks(&scan("x/x/x/x/x/", "never a lover dreams upon the town")),
            "/x x /x / x/ x /"
        );
        // BEAUTIFUL's last syllable can't be promoted beside DREAM, which
        // scans unstressed on its offbeat.
        assert_eq!(marks(&scan("x/x/x", "a beautiful dream")), "x /xx x");
    }

    #[test]
    fn an_ambiguous_syllable_stays_ambiguous_where_no_position_decides() {
        // BLUEBIRD is /\, its secondary stress beside the primary.
        assert_eq!(marks(&scan("___", "the bluebird")), "? /?");
        // WORLD is extra.
        assert_eq!(marks(&scan("x/", "hello world")), "x/ ?");
    }

    #[test]
    fn gaps_come_before_the_word_they_precede() {
        // The line without its second "a".
        let scansion = scan("x/x/x/x/x/", "a lover and summer evening light");
        let gaps: Vec<(usize, Gap)> = scansion
            .words
            .iter()
            .enumerate()
            .flat_map(|(word_index, word)| {
                word.gaps_before.iter().map(move |&gap| (word_index, gap))
            })
            .collect();
        assert_eq!(
            gaps,
            [(
                3,
                Gap {
                    index: 4,
                    expected: U,
                }
            )]
        );
        assert!(scansion.trailing_gaps.is_empty());

        assert_eq!(
            scan("x///", "hello").trailing_gaps,
            [
                Gap {
                    index: 2,
                    expected: S,
                },
                Gap {
                    index: 3,
                    expected: S,
                },
            ]
        );
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

    #[test]
    fn syllable_counts_are_capped() {
        for ok in ["100", "0-100"] {
            assert!(ok.parse::<SyllableCountSpecification>().is_ok(), "{ok}");
        }
        for too_large in ["101", "8-101", "100000000"] {
            let err = too_large.parse::<SyllableCountSpecification>().unwrap_err();
            assert!(
                matches!(err, ParseSyllableCountError::TooLarge),
                "{too_large}"
            );
        }
        assert_eq!(
            ParseSyllableCountError::TooLarge.to_string(),
            "syllable count must be at most 100"
        );
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

        assert!(matches!(
            error,
            ParseSyllableCountSchemeError::Line {
                line: 1,
                source: ParseSyllableCountError::InvalidRange,
            }
        ));
        assert_eq!(error.to_string(), "line 2: min must be <= max");
    }

    #[test]
    fn meter_schemes_need_at_least_one_line() {
        for blank in ["", "\n", " \n\t\n"] {
            let stress = blank.parse::<MeterScheme>().unwrap_err();
            assert!(matches!(stress, ParseMeterSchemeError::Empty), "{blank:?}");
            let counts = MeterScheme::from_syllable_counts(blank).unwrap_err();
            assert!(
                matches!(counts, ParseSyllableCountSchemeError::Empty),
                "{blank:?}"
            );
        }
        assert_eq!(
            ParseMeterSchemeError::Empty.to_string(),
            "meter scheme is empty"
        );
        assert_eq!(
            ParseSyllableCountSchemeError::Empty.to_string(),
            "syllable count scheme is empty"
        );
    }

    #[test]
    fn meter_schemes_skip_blank_lines() {
        // Two quatrain-style lines separated by a blank one: the poem's second
        // line takes the scheme's second pattern, not the blank line.
        let stress: MeterScheme = "x/x/\n\n  \n/x/x".parse().unwrap();
        assert_eq!(stress.scheme.len(), 2);
        assert_meter_matches(stress.check_line(&Line::new("hello hello", dict()), 0));
        assert_meter_matches(stress.check_line(&Line::new("happy summer", dict()), 1));
        assert_meter_matches(stress.check_line(&Line::new("hello hello", dict()), 2));

        let counts = MeterScheme::from_syllable_counts("2\n\n3").unwrap();
        assert_eq!(counts.scheme.len(), 2);
        assert_meter_matches(counts.check_line(&Line::new("cat dog bird", dict()), 1));
    }

    #[test]
    fn scheme_errors_number_lines_as_written() {
        let stress = "x/x/\n\nx/j/".parse::<MeterScheme>().unwrap_err();
        assert_eq!(
            stress.to_string(),
            "line 3: invalid stress character 'j' at column 3"
        );
        let counts = MeterScheme::from_syllable_counts("5\n\n9-7").unwrap_err();
        assert_eq!(counts.to_string(), "line 3: min must be <= max");
    }

    #[test]
    fn meter_scheme_propagates_line_parse_error() {
        let err = "x/x/\nx/j/".parse::<MeterScheme>().unwrap_err();
        assert!(matches!(
            err,
            ParseMeterSchemeError::Line {
                line: 1,
                source: ParseMeterError::InvalidChar { c: 'j', col: 2 },
            }
        ));
    }
}
