//! Verifying a poem against a repetition template.

use crate::error::EchoError;
use crate::line::Line;

use super::{AlignerKind, EchoDetector, EchoMatch, EchoTemplate};

/// How closely a poem's repeated lines are expected to match. This is an
/// expectation about the poem, not a property of its form: traditional
/// villanelles and pantoums repeat lines nearly verbatim, while many
/// contemporary ones (and most duplexes) rework them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EchoStrictness {
    /// Near-verbatim: a word or an inflection may change.
    #[default]
    Strict,
    /// Reworded: phrases dropped, swapped, or rearranged.
    Loose,
}

impl EchoStrictness {
    pub const ALL: [Self; 2] = [Self::Strict, Self::Loose];

    /// The snake_case name used in JSON and corpus headers.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Loose => "loose",
        }
    }

    /// The pass mark for `aligner`, measured on the labeled corpus under its
    /// tuned config ([`super::EchoConfig::for_aligner`]); with another config
    /// these don't apply. Loose marks are each aligner's best balanced
    /// accuracy on loose poems. Strict poems separate almost perfectly over
    /// a wide range (semi-global 0.41–0.68 across resamples), so strict
    /// marks sit above the best point, for margin. On the corpus:
    ///
    /// | aligner | strictness | threshold | echoes caught | false positives |
    /// |---|---|---|---|---|
    /// | semi-global | strict | 0.50 | 99.1% | 0.3% |
    /// | semi-global | loose | 0.28 | 91.0% | 3.9% |
    /// | Waterman–Eggert | strict | 0.45 | 99.1% | 0.2% |
    /// | Waterman–Eggert | loose | 0.24 | 94.7% | 5.9% |
    pub fn threshold(self, aligner: AlignerKind) -> f32 {
        match (aligner, self) {
            (AlignerKind::SemiGlobal, Self::Strict) => 0.50,
            (AlignerKind::SemiGlobal, Self::Loose) => 0.28,
            (AlignerKind::WatermanEggert, Self::Strict) => 0.45,
            (AlignerKind::WatermanEggert, Self::Loose) => 0.24,
        }
    }
}

impl std::fmt::Display for EchoStrictness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for EchoStrictness {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|strictness| strictness.name() == s)
            .ok_or_else(|| format!("unknown strictness {s:?} (expected strict or loose)"))
    }
}

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
    /// Judged at `strictness`'s threshold for `aligner`. The detector passed
    /// to [`Self::check`] should use `EchoConfig::for_aligner(aligner)`.
    pub fn new(template: EchoTemplate, strictness: EchoStrictness, aligner: AlignerKind) -> Self {
        Self::with_threshold(template, strictness.threshold(aligner), aligner)
    }

    /// Judged at the template's conventional strictness
    /// ([`EchoTemplate::conventional_strictness`]) with the default aligner.
    pub fn conventional(template: EchoTemplate) -> Self {
        let strictness = template.conventional_strictness();
        Self::new(template, strictness, AlignerKind::default())
    }

    /// Judged at an explicit `threshold`, e.g. for a config the measured
    /// thresholds don't apply to.
    pub fn with_threshold(template: EchoTemplate, threshold: f32, aligner: AlignerKind) -> Self {
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
    use crate::echo::EchoConfig;
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
    fn thresholds_follow_strictness_for_every_aligner() {
        for kind in AlignerKind::ALL {
            let (strict, loose) = (
                EchoStrictness::Strict.threshold(kind),
                EchoStrictness::Loose.threshold(kind),
            );
            assert!(0.0 < loose && loose < strict && strict < 1.0, "{kind}");
        }
        for strictness in EchoStrictness::ALL {
            assert_eq!(strictness.name().parse(), Ok(strictness));
        }
        assert!("lax".parse::<EchoStrictness>().is_err());
    }

    #[test]
    fn schemes_take_the_measured_threshold_or_an_explicit_one() {
        let scheme = RepetitionScheme::conventional(EchoTemplate::duplex());
        assert_eq!(scheme.aligner, AlignerKind::SemiGlobal);
        assert_eq!(
            scheme.threshold,
            EchoStrictness::Loose.threshold(AlignerKind::SemiGlobal)
        );
        let scheme = RepetitionScheme::conventional(EchoTemplate::villanelle());
        assert_eq!(
            scheme.threshold,
            EchoStrictness::Strict.threshold(AlignerKind::SemiGlobal)
        );
        let scheme = RepetitionScheme::new(
            EchoTemplate::pantoum(4),
            EchoStrictness::Loose,
            AlignerKind::WatermanEggert,
        );
        assert_eq!(
            scheme.threshold,
            EchoStrictness::Loose.threshold(AlignerKind::WatermanEggert)
        );
        let scheme =
            RepetitionScheme::with_threshold(EchoTemplate::blues(3), 0.7, AlignerKind::SemiGlobal);
        assert_eq!(scheme.threshold, 0.7);
    }

    #[test]
    fn a_duplex_passes_its_template() {
        let scheme = RepetitionScheme::conventional(EchoTemplate::duplex());
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
    fn a_duplex_passes_strictly_too_and_under_waterman_eggert() {
        // These followers change one word each, so they are strict echoes.
        let scheme = RepetitionScheme::new(
            EchoTemplate::duplex(),
            EchoStrictness::Strict,
            AlignerKind::SemiGlobal,
        );
        assert!(scheme.check(&lines(&DUPLEX), detector()).passed);
        let lenient =
            EchoDetector::with_config(EchoConfig::for_aligner(AlignerKind::WatermanEggert));
        for strictness in EchoStrictness::ALL {
            let scheme = RepetitionScheme::new(
                EchoTemplate::duplex(),
                strictness,
                AlignerKind::WatermanEggert,
            );
            assert!(
                scheme.check(&lines(&DUPLEX), &lenient).passed,
                "{strictness}"
            );
        }
    }

    #[test]
    fn a_short_duplex_is_missing_its_closing_pair() {
        let scheme = RepetitionScheme::conventional(EchoTemplate::duplex());
        let result = scheme.check(&lines(&DUPLEX[..13]), detector());
        assert_eq!(result.missing_pairs, [(0, 13)]);
        assert!(!result.line_count_matches);
        assert!(!result.passed);
    }

    #[test]
    fn unrelated_followers_fail() {
        let mut texts = DUPLEX;
        texts[2] = "potatoes boil in copper pots";
        let scheme = RepetitionScheme::conventional(EchoTemplate::duplex());
        let result = scheme.check(&lines(&texts), detector());
        let pair = result.pairs.iter().find(|p| p.follower == 2).unwrap();
        assert!(!pair.outcome.as_ref().unwrap().passed);
        assert!(!result.passed);
    }

    #[test]
    fn an_empty_line_is_an_error_not_a_failure() {
        let mut texts = DUPLEX;
        texts[4] = "";
        let scheme = RepetitionScheme::conventional(EchoTemplate::duplex());
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

    /// The fixture under each aligner's tuned config, at its shipped
    /// thresholds: verbatim repeats score 1; every echo passes as loose
    /// and every non-echo fails as strict; unrelated lines fail even as
    /// loose. Only semi-global's known weakness is exempt from passing as
    /// strict: three-block permutations, which strict order can't follow.
    /// (Same-skeleton non-echoes may pass as loose.)
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fixture_pairs_pass_at_the_shipped_thresholds() {
        let pairs = fixture_pairs();
        assert!(pairs.len() >= 40);
        for kind in AlignerKind::ALL {
            let detector = EchoDetector::with_config(EchoConfig::for_aligner(kind));
            let (strict, loose) = (
                EchoStrictness::Strict.threshold(kind),
                EchoStrictness::Loose.threshold(kind),
            );
            for (group, class, expected, a, b) in &pairs {
                let similarity = detector
                    .compare_lines_with(kind, &Line::new(a, dict()), &Line::new(b, dict()))
                    .unwrap_or_else(|e| panic!("{group}: {a:?} / {b:?}: {e}"))
                    .alignment
                    .similarity;
                let context = format!("{kind} {group}: {a:?} / {b:?} = {similarity:.3}");
                if *class == 1 {
                    assert!((similarity - 1.0).abs() < 1e-6, "{context}");
                }
                match expected.as_str() {
                    "echo" => {
                        assert!(similarity >= loose, "loose: {context}");
                        let exempt = kind == AlignerKind::SemiGlobal && *class == 6;
                        assert!(exempt || similarity >= strict, "strict: {context}");
                    }
                    "not" => {
                        assert!(similarity < strict, "strict: {context}");
                        if group == "unrelated" {
                            assert!(similarity < loose, "loose: {context}");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
