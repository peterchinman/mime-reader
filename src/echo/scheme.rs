//! Verifying a poem against a repetition template.

use crate::error::EchoError;
use crate::line::Line;

use super::{AlignerKind, EchoDetector, EchoMatch, EchoTemplate};

/// A placeholder pass mark for normalized echo similarity, until the
/// comparison harness picks one per aligner.
pub const DEFAULT_ECHO_THRESHOLD: f32 = 0.5;

/// A template plus how to judge it: the minimum similarity a follower must
/// reach against its leader, and the aligner that measures it.
#[derive(Debug, Clone, PartialEq)]
pub struct RepetitionScheme {
    pub template: EchoTemplate,
    pub threshold: f32,
    pub aligner: AlignerKind,
}

/// One follower's comparison with its leader.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoPairScore {
    pub alignment: EchoMatch,
    /// Whether the similarity reached the threshold.
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EchoPairResult {
    pub leader: usize,
    pub follower: usize,
    pub outcome: Result<EchoPairScore, EchoError>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EchoFormResult {
    /// Every (leader, follower) pair present in the poem, in template order.
    pub pairs: Vec<EchoPairResult>,
    /// Template pairs the poem is too short to contain.
    pub missing_pairs: Vec<(usize, usize)>,
    /// Whether the poem has the form's line count (true when it has none).
    pub line_count_matches: bool,
    /// Every pair scored and passed, none missing, and the count matches.
    pub passed: bool,
}

impl RepetitionScheme {
    pub fn new(template: EchoTemplate, threshold: f32, aligner: AlignerKind) -> Self {
        Self {
            template,
            threshold,
            aligner,
        }
    }

    /// Compares one follower with its leader.
    pub fn check_pair(
        &self,
        leader: &Line,
        follower: &Line,
        detector: &EchoDetector,
    ) -> Result<EchoPairScore, EchoError> {
        let alignment = detector.compare_lines_with(self.aligner, leader, follower)?;
        let passed = alignment.alignment.similarity >= self.threshold;
        Ok(EchoPairScore { alignment, passed })
    }

    /// Checks every pair the template asserts. `lines` are the poem's
    /// non-blank lines.
    pub fn check(&self, lines: &[Line], detector: &EchoDetector) -> EchoFormResult {
        let mut pairs = Vec::new();
        let mut missing_pairs = Vec::new();
        for (leader, follower) in self.template.pairs() {
            match (lines.get(leader), lines.get(follower)) {
                (Some(leader_line), Some(follower_line)) => pairs.push(EchoPairResult {
                    leader,
                    follower,
                    outcome: self.check_pair(leader_line, follower_line, detector),
                }),
                _ => missing_pairs.push((leader, follower)),
            }
        }
        let line_count_matches = self
            .template
            .expected_line_count
            .is_none_or(|expected| expected == lines.len());
        let passed = line_count_matches
            && missing_pairs.is_empty()
            && pairs
                .iter()
                .all(|pair| pair.outcome.as_ref().is_ok_and(|score| score.passed));
        EchoFormResult {
            pairs,
            missing_pairs,
            line_count_matches,
            passed,
        }
    }

    /// The leader `line` follows, if it is a follower.
    #[allow(dead_code)] // for the deferred `Poem` integration
    pub(crate) fn leader_of(&self, line: usize) -> Option<usize> {
        self.template.leader_of(line)
    }

    /// The followers that must be re-checked when `leader` changes.
    #[allow(dead_code)] // for the deferred `Poem` integration
    pub(crate) fn dependent_follower_indices(&self, leader: usize) -> Vec<usize> {
        self.template.dependent_follower_indices(leader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo::test_support::{detector, dict};

    fn lines(texts: &[&str]) -> Vec<Line> {
        texts.iter().map(|t| Line::new(t, dict())).collect()
    }

    const DUPLEX: [&str; 14] = [
        "a hand I hold across the river",
        "a light that leaves the window",
        "a light that left the window",
        "my mother calls the dog inside",
        "my mother called the dogs inside",
        "the road is long and narrow",
        "the road was long and narrow",
        "we sang until the morning",
        "we sang into the morning",
        "the bread is warm and broken",
        "the bread was warm and broken",
        "nobody knows the river",
        "nobody knew the river",
        "a hand I held across the river",
    ];

    #[test]
    fn a_duplex_passes_its_template() {
        let scheme =
            RepetitionScheme::new(EchoTemplate::duplex(), 0.6, AlignerKind::WatermanEggert);
        let result = scheme.check(&lines(&DUPLEX), detector());
        assert!(result.line_count_matches);
        assert!(result.missing_pairs.is_empty());
        assert_eq!(result.pairs.len(), 7);
        for pair in &result.pairs {
            let score = pair.outcome.as_ref().unwrap();
            assert!(
                score.passed,
                "{} → {}: {}",
                pair.leader, pair.follower, score.alignment.alignment.similarity
            );
        }
        assert!(result.passed);
    }

    #[test]
    fn a_short_duplex_is_missing_its_closing_pair() {
        let scheme = RepetitionScheme::new(EchoTemplate::duplex(), 0.6, AlignerKind::SemiGlobal);
        let result = scheme.check(&lines(&DUPLEX[..13]), detector());
        assert_eq!(result.missing_pairs, [(0, 13)]);
        assert!(!result.line_count_matches);
        assert!(!result.passed);
    }

    #[test]
    fn unrelated_followers_fail() {
        let mut texts = DUPLEX;
        texts[2] = "potatoes boil in copper pots";
        let scheme = RepetitionScheme::new(EchoTemplate::duplex(), 0.6, AlignerKind::Jump);
        let result = scheme.check(&lines(&texts), detector());
        let pair = result.pairs.iter().find(|p| p.follower == 2).unwrap();
        assert!(!pair.outcome.as_ref().unwrap().passed);
        assert!(!result.passed);
    }

    #[test]
    fn an_empty_line_is_an_error_not_a_failure() {
        let mut texts = DUPLEX;
        texts[4] = "";
        let scheme = RepetitionScheme::new(EchoTemplate::duplex(), 0.6, AlignerKind::SemiGlobal);
        let result = scheme.check(&lines(&texts), detector());
        let pair = result.pairs.iter().find(|p| p.follower == 4).unwrap();
        assert_eq!(pair.outcome, Err(EchoError::EmptyLine));
        assert!(!result.passed);
    }

    /// The synthetic fixture pairs in `echo_pair_tests.json`, as
    /// (group, class, expected, a, b). (serde_json is a native-only
    /// dev-dependency.)
    #[cfg(not(target_arch = "wasm32"))]
    fn fixture_pairs() -> Vec<(String, u64, String, String, String)> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/echo_pair_tests.json");
        let text = std::fs::read_to_string(path).expect("echo_pair_tests.json is readable");
        let json: serde_json::Value =
            serde_json::from_str(&text).expect("echo_pair_tests.json parses");
        let mut pairs = Vec::new();
        for (group, entry) in json.as_object().expect("an object of groups") {
            let class = entry["class"].as_u64().expect("class");
            let expected = entry["expected"].as_str().expect("expected").to_string();
            for pair in entry["pairs"].as_array().expect("pairs") {
                let a = pair[0].as_str().expect("leader").to_string();
                let b = pair[1].as_str().expect("follower").to_string();
                pairs.push((group.clone(), class, expected.clone(), a, b));
            }
        }
        pairs
    }

    /// A loose regression floor for every aligner, to be tightened once the
    /// comparison harness picks an aligner and threshold: verbatim pairs
    /// score ~1, and echoes outscore non-echoes on average.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fixture_pairs_separate_for_every_aligner() {
        let pairs = fixture_pairs();
        assert!(pairs.len() >= 40);
        for kind in AlignerKind::ALL {
            let (mut positives, mut negatives) = (Vec::new(), Vec::new());
            for (group, class, expected, a, b) in &pairs {
                let echo = detector()
                    .compare_lines_with(kind, &Line::new(a, dict()), &Line::new(b, dict()))
                    .unwrap_or_else(|e| panic!("{group}: {a:?} / {b:?}: {e}"));
                let similarity = echo.alignment.similarity;
                if *class == 1 {
                    assert!(
                        similarity >= 0.99,
                        "{kind} {group}: {a:?} / {b:?} = {similarity}"
                    );
                }
                match expected.as_str() {
                    "echo" => positives.push(similarity),
                    "not" => negatives.push(similarity),
                    _ => {}
                }
            }
            let mean = |xs: &[f32]| xs.iter().sum::<f32>() / xs.len() as f32;
            assert!(
                mean(&positives) > mean(&negatives),
                "{kind}: mean echo {} ≤ mean non-echo {}",
                mean(&positives),
                mean(&negatives)
            );
        }
    }
}
