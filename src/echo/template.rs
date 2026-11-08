//! Which lines of a form repeat which.

use std::collections::BTreeMap;

use super::EchoStrictness;
use crate::error::ParseEchoTemplateError;

/// The longest template, in lines: far beyond any repeating form, small
/// enough that a typo'd `*N` can't exhaust memory.
pub const MAX_TEMPLATE_LINES: usize = 1000;

/// A repetition template: groups of line indices (0-based, blank lines not
/// counted) whose lines echo each other. The first line of a group leads;
/// the rest follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EchoTemplate {
    name: String,
    /// Each group sorted, with at least two lines; groups ordered by their
    /// first line; no line in two groups.
    groups: Vec<Vec<usize>>,
    expected_line_count: Option<usize>,
    strictness: EchoStrictness,
}

impl EchoTemplate {
    /// A template from explicit groups, conventionally strict. Groups of
    /// fewer than two lines are dropped; a line in two groups is an error,
    /// as is a template with no group left or a line index (or line count)
    /// of [`MAX_TEMPLATE_LINES`] or more.
    pub fn new(
        name: impl Into<String>,
        groups: Vec<Vec<usize>>,
        expected_line_count: Option<usize>,
    ) -> Result<Self, ParseEchoTemplateError> {
        let too_long = |n: usize| n > MAX_TEMPLATE_LINES;
        if expected_line_count.is_some_and(too_long)
            || groups.iter().flatten().any(|&line| too_long(line + 1))
        {
            return Err(ParseEchoTemplateError::TooLong {
                max: MAX_TEMPLATE_LINES,
            });
        }
        let mut groups: Vec<Vec<usize>> = groups
            .into_iter()
            .map(|mut group| {
                group.sort_unstable();
                group.dedup();
                group
            })
            .filter(|group| group.len() >= 2)
            .collect();
        groups.sort();
        let mut seen = std::collections::BTreeSet::new();
        for &line in groups.iter().flatten() {
            if !seen.insert(line) {
                return Err(ParseEchoTemplateError::OverlappingGroups { index: line });
            }
        }
        if groups.is_empty() {
            return Err(ParseEchoTemplateError::NoEchoes);
        }
        Ok(Self {
            name: name.into(),
            groups,
            expected_line_count,
            strictness: EchoStrictness::Strict,
        })
    }

    /// The same template, conventionally judged at `strictness`.
    pub fn with_strictness(mut self, strictness: EchoStrictness) -> Self {
        self.strictness = strictness;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The groups of lines that echo each other: each sorted, with at least
    /// two lines; ordered by their first line; no line in two groups.
    pub fn groups(&self) -> &[Vec<usize>] {
        &self.groups
    }

    /// The form's line count, when it has one.
    pub fn expected_line_count(&self) -> Option<usize> {
        self.expected_line_count
    }

    /// How closely the form conventionally repeats its lines: loose for the
    /// duplex, which reworks its echoes by design; strict for the other
    /// built-in forms and, unless set with [`Self::with_strictness`], for
    /// custom templates. A default only — a contemporary villanelle or
    /// pantoum may well be loose.
    pub fn strictness(&self) -> EchoStrictness {
        self.strictness
    }

    fn known(name: &str, groups: Vec<Vec<usize>>, lines: usize) -> Self {
        Self::new(name, groups, Some(lines)).expect("built-in templates are valid")
    }

    /// `stanzas` repetitions of a `lines_per_stanza`-line stanza, checked
    /// against [`MAX_TEMPLATE_LINES`]; 0 stanzas has nothing to echo.
    fn stanza_lines(
        stanzas: usize,
        lines_per_stanza: usize,
    ) -> Result<usize, ParseEchoTemplateError> {
        if stanzas == 0 {
            return Err(ParseEchoTemplateError::NoEchoes);
        }
        stanzas
            .checked_mul(lines_per_stanza)
            .filter(|&lines| lines <= MAX_TEMPLATE_LINES)
            .ok_or(ParseEchoTemplateError::TooLong {
                max: MAX_TEMPLATE_LINES,
            })
    }

    /// Jericho Brown's duplex: 14 lines in couplets; the last line returns
    /// to the first, and each couplet's second line is echoed by the next
    /// couplet's first. Conventionally loose.
    pub fn duplex() -> Self {
        let mut groups = vec![vec![0, 13]];
        groups.extend((0..6).map(|k| vec![2 * k + 1, 2 * k + 2]));
        Self::known("duplex", groups, 14).with_strictness(EchoStrictness::Loose)
    }

    /// 19 lines: refrain A1 at lines 1, 6, 12, 18 and A2 at 3, 9, 15, 19
    /// (1-based).
    pub fn villanelle() -> Self {
        Self::known(
            "villanelle",
            vec![vec![0, 5, 11, 17], vec![2, 8, 14, 18]],
            19,
        )
    }

    /// `stanzas` quatrains: each stanza's 2nd and 4th lines become the next
    /// stanza's 1st and 3rd; the last stanza's 2nd and 4th return to the
    /// first stanza's 3rd and 1st.
    pub fn pantoum(stanzas: usize) -> Result<Self, ParseEchoTemplateError> {
        let lines = Self::stanza_lines(stanzas, 4)?;
        let mut groups = Vec::new();
        for k in 0..stanzas.saturating_sub(1) {
            groups.push(vec![4 * k + 1, 4 * k + 4]);
            groups.push(vec![4 * k + 3, 4 * k + 6]);
        }
        let last = 4 * (stanzas - 1);
        groups.push(vec![2, last + 1]);
        groups.push(vec![0, last + 3]);
        Self::new("pantoum", groups, Some(lines))
    }

    /// `stanzas` AAB tercets: each stanza's first line repeated as its
    /// second.
    pub fn blues(stanzas: usize) -> Result<Self, ParseEchoTemplateError> {
        let lines = Self::stanza_lines(stanzas, 3)?;
        let groups = (0..stanzas).map(|k| vec![3 * k, 3 * k + 1]).collect();
        Self::new("blues", groups, Some(lines))
    }

    /// Parses a template with one character per line: a letter marks lines
    /// that echo each other, `_` a free line; whitespace is ignored. `*N`
    /// repeats everything since the previous `*N` (or the start) `N` times.
    /// UPPERCASE letters name the same group everywhere; lowercase letters
    /// are fresh in each repetition. So `aab*4` is four AAB stanzas, each
    /// with its own refrain, while `AAB*4` is one refrain and one B line
    /// shared by all four.
    ///
    /// ```
    /// use mime_reader::echo::EchoTemplate;
    /// let villanelle = EchoTemplate::parse("A_B__A__B__A__B__AB")?;
    /// assert_eq!(villanelle.groups(), EchoTemplate::villanelle().groups());
    /// # Ok::<(), mime_reader::ParseEchoTemplateError>(())
    /// ```
    pub fn parse(s: &str) -> Result<Self, ParseEchoTemplateError> {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Key {
            Global(char),
            Local {
                unit: usize,
                repetition: usize,
                letter: char,
            },
        }

        let mut groups: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
        let mut line = 0;
        // The current unit's slots: Some(letter) or None for a free line.
        let mut unit: Vec<Option<char>> = Vec::new();
        let mut unit_index = 0;
        let mut expand =
            |unit: &[Option<char>], repetitions: usize, unit_index: usize, line: &mut usize| {
                for repetition in 0..repetitions {
                    for slot in unit {
                        if let Some(letter) = *slot {
                            let key = if letter.is_ascii_uppercase() {
                                Key::Global(letter)
                            } else {
                                Key::Local {
                                    unit: unit_index,
                                    repetition,
                                    letter,
                                }
                            };
                            groups.entry(key).or_default().push(*line);
                        }
                        *line += 1;
                    }
                }
            };

        let chars: Vec<char> = s.chars().collect();
        let mut col = 0;
        while col < chars.len() {
            let c = chars[col];
            match c {
                c if c.is_whitespace() => {}
                '_' => unit.push(None),
                c if c.is_ascii_alphabetic() => unit.push(Some(c)),
                '*' => {
                    let digits: String = chars[col + 1..]
                        .iter()
                        .take_while(|d| d.is_ascii_digit())
                        .collect();
                    let repetitions: usize = digits
                        .parse()
                        .ok()
                        .filter(|&n| n > 0)
                        .ok_or(ParseEchoTemplateError::InvalidRepeat { col })?;
                    if unit.is_empty() {
                        return Err(ParseEchoTemplateError::InvalidRepeat { col });
                    }
                    let expanded = unit
                        .len()
                        .checked_mul(repetitions)
                        .and_then(|n| n.checked_add(line));
                    if expanded.is_none_or(|n| n > MAX_TEMPLATE_LINES) {
                        return Err(ParseEchoTemplateError::TooLong {
                            max: MAX_TEMPLATE_LINES,
                        });
                    }
                    expand(&unit, repetitions, unit_index, &mut line);
                    unit.clear();
                    unit_index += 1;
                    col += digits.len();
                }
                c => return Err(ParseEchoTemplateError::InvalidChar { c, col }),
            }
            col += 1;
        }
        expand(&unit, 1, unit_index, &mut line);

        if line == 0 {
            return Err(ParseEchoTemplateError::Empty);
        }
        Self::new(s.trim(), groups.into_values().collect(), Some(line))
    }

    /// (leader, follower) for every follower: each group's first line leads.
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        self.groups
            .iter()
            .flat_map(|group| group[1..].iter().map(move |&follower| (group[0], follower)))
            .collect()
    }

    /// Every (i, j), i < j, of lines in the same group: all the echoes the
    /// form asserts.
    pub fn all_pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs: Vec<(usize, usize)> = self
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .iter()
                    .enumerate()
                    .flat_map(move |(x, &i)| group[x + 1..].iter().map(move |&j| (i, j)))
            })
            .collect();
        pairs.sort_unstable();
        pairs
    }

    /// The leader `line` follows, if it is a follower.
    #[allow(dead_code)] // for the deferred `Poem` integration
    pub(crate) fn leader_of(&self, line: usize) -> Option<usize> {
        self.groups
            .iter()
            .find(|group| group[1..].contains(&line))
            .map(|group| group[0])
    }

    /// The lines that follow `leader` (empty if it leads no group).
    #[allow(dead_code)] // for the deferred `Poem` integration
    pub(crate) fn dependent_follower_indices(&self, leader: usize) -> Vec<usize> {
        self.groups
            .iter()
            .find(|group| group[0] == leader)
            .map(|group| group[1..].to_vec())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_build_the_standard_groups() {
        assert_eq!(
            EchoTemplate::duplex().groups(),
            [
                vec![0, 13],
                vec![1, 2],
                vec![3, 4],
                vec![5, 6],
                vec![7, 8],
                vec![9, 10],
                vec![11, 12]
            ]
        );
        assert_eq!(EchoTemplate::villanelle().expected_line_count(), Some(19));
        assert_eq!(
            EchoTemplate::pantoum(2).unwrap().pairs(),
            [(0, 7), (1, 4), (2, 5), (3, 6)]
        );
        assert_eq!(
            EchoTemplate::pantoum(2).unwrap().expected_line_count(),
            Some(8)
        );
        assert_eq!(
            EchoTemplate::blues(2).unwrap().groups(),
            [vec![0, 1], vec![3, 4]]
        );
    }

    #[test]
    fn parse_round_trips_the_constructors() {
        let cases = [
            ("A_B__A__B__A__B__AB", EchoTemplate::villanelle()),
            ("aab*4", EchoTemplate::blues(4).unwrap()),
            ("A BB CC DD EE FF GG A", EchoTemplate::duplex()),
            ("ABCD BEDF EGFH GCHA", EchoTemplate::pantoum(4).unwrap()),
        ];
        for (scheme, expected) in cases {
            let parsed = EchoTemplate::parse(scheme).unwrap();
            assert_eq!(parsed.groups(), expected.groups(), "{scheme}");
            assert_eq!(
                parsed.expected_line_count(),
                expected.expected_line_count(),
                "{scheme}"
            );
        }
    }

    #[test]
    fn case_scopes_letters_to_one_repetition_or_all() {
        let local = EchoTemplate::parse("aab*4").unwrap();
        assert_eq!(local.groups().len(), 4);
        let global = EchoTemplate::parse("AAB*4").unwrap();
        assert_eq!(
            global.groups(),
            [vec![0, 1, 3, 4, 6, 7, 9, 10], vec![2, 5, 8, 11]]
        );
        // A refrain shared by repeated stanzas, with a lowercase couplet
        // fresh in each.
        let mixed = EchoTemplate::parse("abab__*6").unwrap();
        assert_eq!(mixed.expected_line_count(), Some(36));
        assert_eq!(mixed.groups().len(), 12);
        assert_eq!(mixed.groups()[1], [1, 3]);
        let mariana = EchoTemplate::parse("________ABCD*7").unwrap();
        assert_eq!(mariana.expected_line_count(), Some(84));
        assert_eq!(mariana.groups()[0], [8, 20, 32, 44, 56, 68, 80]);
        // Units after a repeat start fresh.
        let units = EchoTemplate::parse("aa*2 aa").unwrap();
        assert_eq!(units.groups(), [vec![0, 1], vec![2, 3], vec![4, 5]]);
    }

    #[test]
    fn pairs_lead_from_each_groups_first_line() {
        let villanelle = EchoTemplate::villanelle();
        assert_eq!(villanelle.pairs().len(), 6);
        assert_eq!(villanelle.all_pairs().len(), 12);
        assert_eq!(villanelle.leader_of(11), Some(0));
        assert_eq!(villanelle.leader_of(0), None);
        assert_eq!(villanelle.dependent_follower_indices(2), [8, 14, 18]);
        assert!(villanelle.dependent_follower_indices(1).is_empty());
    }

    #[test]
    fn parse_errors_name_their_column() {
        assert_eq!(
            EchoTemplate::parse("   "),
            Err(ParseEchoTemplateError::Empty)
        );
        assert_eq!(
            EchoTemplate::parse("AB-A"),
            Err(ParseEchoTemplateError::InvalidChar { c: '-', col: 2 })
        );
        assert_eq!(
            EchoTemplate::parse("aab*"),
            Err(ParseEchoTemplateError::InvalidRepeat { col: 3 })
        );
        assert_eq!(
            EchoTemplate::parse("aab*0"),
            Err(ParseEchoTemplateError::InvalidRepeat { col: 3 })
        );
        assert_eq!(
            EchoTemplate::parse("aa *2 *3"),
            Err(ParseEchoTemplateError::InvalidRepeat { col: 6 })
        );
        assert_eq!(
            EchoTemplate::parse("AB__"),
            Err(ParseEchoTemplateError::NoEchoes)
        );
        assert_eq!(
            EchoTemplate::new("x", vec![vec![0, 2], vec![2, 4]], None),
            Err(ParseEchoTemplateError::OverlappingGroups { index: 2 })
        );
    }

    #[test]
    fn templates_are_bounded() {
        let too_long = Err(ParseEchoTemplateError::TooLong {
            max: MAX_TEMPLATE_LINES,
        });
        assert_eq!(
            EchoTemplate::pantoum(0),
            Err(ParseEchoTemplateError::NoEchoes)
        );
        assert_eq!(
            EchoTemplate::blues(0),
            Err(ParseEchoTemplateError::NoEchoes)
        );
        assert_eq!(EchoTemplate::pantoum(MAX_TEMPLATE_LINES / 4 + 1), too_long);
        assert_eq!(EchoTemplate::blues(usize::MAX), too_long);
        assert_eq!(EchoTemplate::parse("aa*200000000"), too_long);
        assert_eq!(EchoTemplate::parse(&format!("aa*{}", usize::MAX)), too_long);
        assert_eq!(
            EchoTemplate::new("x", vec![vec![0, MAX_TEMPLATE_LINES]], None),
            too_long
        );
        let longest = EchoTemplate::parse(&format!("aa*{}", MAX_TEMPLATE_LINES / 2)).unwrap();
        assert_eq!(longest.expected_line_count(), Some(MAX_TEMPLATE_LINES));
    }

    #[test]
    fn only_the_duplex_is_conventionally_loose() {
        assert_eq!(EchoTemplate::duplex().strictness(), EchoStrictness::Loose);
        assert_eq!(
            EchoTemplate::villanelle().strictness(),
            EchoStrictness::Strict
        );
        // A duplex scheme typed by hand is a custom template: strict unless
        // said otherwise.
        let custom = EchoTemplate::parse("A BB CC DD EE FF GG A").unwrap();
        assert_eq!(custom.groups(), EchoTemplate::duplex().groups());
        assert_eq!(custom.strictness(), EchoStrictness::Strict);
        let custom = custom.with_strictness(EchoStrictness::Loose);
        assert_eq!(custom.strictness(), EchoStrictness::Loose);
    }
}
