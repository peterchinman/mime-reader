//! Phoneme deformations: rewrite a line's phonemes so that, read back as
//! General American, it sounds the way an accent (or an arbitrary knob)
//! would make it sound.
//!
//! A [`Deformation`] is a list of [`Step`]s applied in order, each to every
//! word:
//! - [`Rule`]: a context-sensitive rewrite of one phoneme class: replace,
//!   delete, insert, voice or devoice. "TH → T anywhere", "R → ∅ when no
//!   vowel follows", "final voiced obstruents devoice".
//! - [`VowelMove`]: a geometric operation on the vowel chart (shift, pull
//!   toward a point, rotate, mirror, scale), after which each vowel snaps to
//!   the nearest ARPAbet vowel. Diphthongs move both their start and end.
//!   These are the "knobs": "pull vowels toward the top right" is
//!   `Pull { toward: (1.0, 1.0), amount }`.
//!
//! - Word-level steps such as [`StepKind::FinalStress`].
//!
//! Every step has a `rate` in 0–1: the share of matching sites it applies
//! to, chosen by a seeded hash of the word and position (so the same input
//! always deforms the same way). Rules may also depend on a word's
//! spelling, for features no phoneme context predicts (stage-Irish "tay"
//! for *tea* is IY spelled ‹ea›), and an [`Action::Map`] rewrites several
//! phonemes in one pass, as chain shifts need (New Zealand TRAP → DRESS,
//! DRESS → KIT must not cascade TRAP into KIT).

use crate::phone::{Manner, Ph};

/// Which phonemes a rule or context matches.
#[derive(Debug, Clone, PartialEq)]
pub enum Class {
    /// These symbols, without stress: `["TH", "DH"]`, `["AY"]`.
    Is(Vec<&'static str>),
    /// These vowels, stressed (1 or 2): STRUT is `StressedIs(["AH"])`.
    StressedIs(Vec<&'static str>),
    /// These vowels, unstressed.
    UnstressedIs(Vec<&'static str>),
    Vowel,
    StressedVowel,
    UnstressedVowel,
    Consonant,
    Manner(Manner),
    /// Voiced stops, fricatives and affricates.
    VoicedObstruent,
    /// Voiceless stops, fricatives and affricates.
    VoicelessObstruent,
}

impl Class {
    pub fn is(symbols: &[&'static str]) -> Self {
        Self::Is(symbols.to_vec())
    }

    pub fn matches(&self, p: Ph) -> bool {
        let obstruent = matches!(
            p.manner(),
            Some(Manner::Stop | Manner::Fricative | Manner::Affricate)
        );
        match self {
            Self::Is(symbols) => symbols.contains(&p.symbol()),
            Self::StressedIs(symbols) => p.is_stressed() && symbols.contains(&p.symbol()),
            Self::UnstressedIs(symbols) => p.stress() == Some(0) && symbols.contains(&p.symbol()),
            Self::Vowel => p.is_vowel(),
            Self::StressedVowel => p.is_stressed(),
            Self::UnstressedVowel => p.stress() == Some(0),
            Self::Consonant => !p.is_vowel(),
            Self::Manner(m) => p.manner() == Some(*m),
            Self::VoicedObstruent => obstruent && p.is_voiced(),
            Self::VoicelessObstruent => obstruent && !p.is_voiced(),
        }
    }
}

/// What must be next to the target, on one side.
#[derive(Debug, Clone, PartialEq)]
pub enum Context {
    Any,
    /// The word boundary.
    Edge,
    /// A phoneme of this class.
    Class(Class),
    /// Anything but this class, including the word boundary.
    Not(Class),
}

impl Context {
    fn matches(&self, neighbor: Option<Ph>) -> bool {
        match (self, neighbor) {
            (Self::Any, _) => true,
            (Self::Edge, None) => true,
            (Self::Edge, Some(_)) => false,
            (Self::Class(c), Some(p)) => c.matches(p),
            (Self::Class(_), None) => false,
            (Self::Not(c), Some(p)) => !c.matches(p),
            (Self::Not(_), None) => true,
        }
    }
}

/// What a rule does to a matching phoneme.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Becomes this symbol; a vowel keeps its stress (a consonant
    /// replacing a vowel, or the reverse, is not allowed).
    To(&'static str),
    /// Becomes these phonemes: symbols, vowels with explicit stress
    /// (`"IH0"`), or `"*"` for the matched phoneme itself.
    Seq(Vec<&'static str>),
    Delete,
    /// Voiced obstruents lose voicing.
    Devoice,
    /// Voiceless obstruents gain voicing.
    Voice,
    /// Each `(from, to)` symbol pair at once, so a chain shift doesn't
    /// cascade (vowels keep their stress).
    Map(Vec<(&'static str, &'static str)>),
}

/// A condition on a word's spelling (lowercase).
#[derive(Debug, Clone, PartialEq)]
pub enum Spelling {
    /// The word contains any of these.
    Contains(Vec<&'static str>),
    /// The word is one of these.
    Word(Vec<&'static str>),
    /// The word is none of these.
    NotWord(Vec<&'static str>),
}

impl Spelling {
    fn matches(&self, word: Option<&str>) -> bool {
        match (self, word) {
            (Self::Contains(parts), Some(w)) => parts.iter().any(|p| w.contains(p)),
            (Self::Word(words), Some(w)) => words.contains(&w),
            (Self::NotWord(words), Some(w)) => !words.contains(&w),
            (Self::NotWord(_), None) => true,
            (_, None) => false,
        }
    }
}

/// A context-sensitive rewrite: `target` → `action` when `left` and
/// `right` match its neighbors in the word.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: &'static str,
    pub target: Class,
    pub left: Context,
    pub right: Context,
    pub action: Action,
    /// Fires only in words spelled this way.
    pub spelled: Option<Spelling>,
}

impl Rule {
    pub fn new(name: &'static str, target: Class, action: Action) -> Self {
        Self {
            name,
            target,
            left: Context::Any,
            right: Context::Any,
            action,
            spelled: None,
        }
    }

    /// Rewrites each `from` symbol to its `to`, all in one pass.
    pub fn map(name: &'static str, pairs: &[(&'static str, &'static str)]) -> Self {
        Self::new(
            name,
            Class::Is(pairs.iter().map(|&(from, _)| from).collect()),
            Action::Map(pairs.to_vec()),
        )
    }

    pub fn spelled(mut self, spelling: Spelling) -> Self {
        self.spelled = Some(spelling);
        self
    }

    pub fn left(mut self, left: Context) -> Self {
        self.left = left;
        self
    }

    pub fn right(mut self, right: Context) -> Self {
        self.right = right;
        self
    }
}

/// Positions on the vowel chart: `x` from front (0) to back (1), `y` from
/// low (0) to high (1).
pub type Point = (f32, f32);

/// A geometric operation on the vowel chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VowelMove {
    Shift {
        dx: f32,
        dy: f32,
    },
    /// Moves only a diphthong's start (a monophthong moves whole): Labov's
    /// "the nuclei of upgliding diphthongs fall" is `ShiftOnset { dy < 0 }`.
    ShiftOnset {
        dx: f32,
        dy: f32,
    },
    /// Moves each point `amount` (0–1) of the way to `toward`.
    Pull {
        toward: Point,
        amount: f32,
    },
    /// Turns the chart about its center; positive is counterclockwise.
    Rotate {
        degrees: f32,
    },
    /// Front ↔ back.
    MirrorFrontBack,
    /// High ↔ low.
    MirrorHeight,
    /// Spreads (> 1) or crowds (< 1) the vowels about the center.
    Scale {
        factor: f32,
    },
}

/// The chart's center: the speakers' vowel-space centroid (the Lobanov
/// origin) in Hillenbrand et al.'s data, where schwa sits.
const CENTER: Point = (0.58, 0.59);

impl VowelMove {
    fn apply(self, (x, y): Point) -> Point {
        let (cx, cy) = CENTER;
        match self {
            Self::Shift { dx, dy } | Self::ShiftOnset { dx, dy } => (x + dx, y + dy),
            Self::Pull { toward, amount } => {
                (x + amount * (toward.0 - x), y + amount * (toward.1 - y))
            }
            Self::Rotate { degrees } => {
                // The chart's x runs front → back, rightward as drawn.
                let (s, c) = degrees.to_radians().sin_cos();
                let (dx, dy) = (x - cx, y - cy);
                (cx + c * dx - s * dy, cy + s * dx + c * dy)
            }
            Self::MirrorFrontBack => (1.0 - x, y),
            Self::MirrorHeight => (x, 1.0 - y),
            Self::Scale { factor } => (cx + factor * (x - cx), cy + factor * (y - cy)),
        }
    }
}

/// A vowel move and which vowels it touches.
#[derive(Debug, Clone, PartialEq)]
pub struct VowelStep {
    pub moves: Vec<VowelMove>,
    /// Leave unstressed vowels (mostly schwa) alone: accents show most in
    /// stressed vowels, and moving schwa mostly adds noise.
    pub stressed_only: bool,
    /// Only these vowels, e.g. [`Class::is`] of the tense ones.
    pub only: Option<Class>,
}

/// The tense ("long") vowels, which Labov finds rise in chain shifts.
pub const TENSE: [&str; 6] = ["IY", "EY", "AA", "AO", "OW", "UW"];
/// The lax ("short") vowels, which fall.
pub const LAX: [&str; 5] = ["IH", "EH", "AE", "UH", "AH"];

#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Rule(Rule),
    Vowels(VowelStep),
    /// Primary stress on each word's last vowel, none elsewhere (French).
    FinalStress,
    /// Rules matched against the word as it was before this step, all at
    /// once: each phoneme is rewritten by the first rule that matches it.
    /// For chain shifts that loop (the Northern Cities Shift sends AE → EH
    /// → AH → AO → AA → AE), where no order of single rules avoids a
    /// cascade.
    Simultaneous(Vec<Rule>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: StepKind,
    /// The share of matching sites this applies to.
    pub rate: f32,
}

/// A sequence of steps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Deformation {
    pub name: String,
    pub steps: Vec<Step>,
    pub seed: u64,
}

impl Deformation {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn rule(mut self, rule: Rule) -> Self {
        self.steps.push(Step {
            kind: StepKind::Rule(rule),
            rate: 1.0,
        });
        self
    }

    pub fn vowels(self, moves: Vec<VowelMove>) -> Self {
        self.vowels_only(moves, None)
    }

    /// Vowel moves for the vowels in `only`.
    pub fn vowels_only(mut self, moves: Vec<VowelMove>, only: Option<Class>) -> Self {
        self.steps.push(Step {
            kind: StepKind::Vowels(VowelStep {
                moves,
                stressed_only: true,
                only,
            }),
            rate: 1.0,
        });
        self
    }

    pub fn step(mut self, kind: StepKind) -> Self {
        self.steps.push(Step { kind, rate: 1.0 });
        self
    }

    /// A [`StepKind::Simultaneous`] step.
    pub fn simultaneous(self, rules: Vec<Rule>) -> Self {
        self.step(StepKind::Simultaneous(rules))
    }

    /// Sets the rate of the last step.
    pub fn at(mut self, rate: f32) -> Self {
        if let Some(step) = self.steps.last_mut() {
            step.rate = rate;
        }
        self
    }

    /// Scales every step's rate by `intensity` (0–1): one knob for how
    /// thick the accent is.
    pub fn intensity(mut self, intensity: f32) -> Self {
        for step in &mut self.steps {
            step.rate = (step.rate * intensity).clamp(0.0, 1.0);
        }
        self
    }

    /// Appends `other`'s steps (keeping this deformation's name).
    pub fn then(mut self, other: Deformation) -> Self {
        self.steps.extend(other.steps);
        if self.name.is_empty() {
            self.name = other.name;
        }
        self
    }

    /// Applies every step to every word (rules that depend on spelling
    /// don't fire: see [`Self::apply_spelled`]).
    pub fn apply(&self, words: &[Vec<Ph>]) -> Vec<Vec<Ph>> {
        self.apply_spelled(words, &[])
    }

    /// Applies every step to every word, with `spellings[w]` the spelling
    /// (lowercase) of word `w`.
    pub fn apply_spelled(&self, words: &[Vec<Ph>], spellings: &[String]) -> Vec<Vec<Ph>> {
        let chart = VowelChart::general_american();
        let mut words: Vec<Vec<Ph>> = words.to_vec();
        for (s, step) in self.steps.iter().enumerate() {
            for (w, word) in words.iter_mut().enumerate() {
                let spelling = spellings.get(w).map(String::as_str);
                let chosen = |position: usize| {
                    step.rate >= 1.0
                        || unit(self.seed ^ (s as u64) << 40 ^ (w as u64) << 20 ^ position as u64)
                            < step.rate
                };
                *word = match &step.kind {
                    StepKind::Rule(rule) => {
                        if rule
                            .spelled
                            .as_ref()
                            .is_some_and(|sp| !sp.matches(spelling))
                        {
                            continue;
                        }
                        apply_rule(rule, word, chosen)
                    }
                    StepKind::FinalStress => final_stress(word),
                    StepKind::Simultaneous(rules) => {
                        let mut out = Vec::with_capacity(word.len() + 1);
                        for (i, &p) in word.iter().enumerate() {
                            let rule = rules.iter().find(|rule| {
                                rule.spelled.as_ref().is_none_or(|sp| sp.matches(spelling))
                                    && matches_at(rule, word, i)
                            });
                            match rule {
                                Some(rule) if chosen(i) => rewrite(rule, p, &mut out),
                                _ => out.push(p),
                            }
                        }
                        out
                    }
                    StepKind::Vowels(vowels) => word
                        .iter()
                        .enumerate()
                        .map(|(i, &p)| {
                            let movable = p.is_vowel()
                                && p.symbol() != "ER"
                                && (!vowels.stressed_only || p.is_stressed())
                                && vowels.only.as_ref().is_none_or(|c| c.matches(p));
                            if movable && chosen(i) {
                                chart.moved(p, &vowels.moves)
                            } else {
                                p
                            }
                        })
                        .collect(),
                };
            }
        }
        words
    }
}

/// A deterministic pseudo-random number in `[0, 1)`.
fn unit(seed: u64) -> f32 {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Whether `rule` matches `word[i]` in its context.
fn matches_at(rule: &Rule, word: &[Ph], i: usize) -> bool {
    let left = i.checked_sub(1).map(|j| word[j]);
    let right = word.get(i + 1).copied();
    rule.target.matches(word[i]) && rule.left.matches(left) && rule.right.matches(right)
}

fn apply_rule(rule: &Rule, word: &[Ph], chosen: impl Fn(usize) -> bool) -> Vec<Ph> {
    let mut out = Vec::with_capacity(word.len() + 1);
    for (i, &p) in word.iter().enumerate() {
        if matches_at(rule, word, i) && chosen(i) {
            rewrite(rule, p, &mut out);
        } else {
            out.push(p);
        }
    }
    out
}

/// Pushes what `rule` turns `p` into.
fn rewrite(rule: &Rule, p: Ph, out: &mut Vec<Ph>) {
    {
        match &rule.action {
            Action::To(symbol) => out.push(replace(p, symbol)),
            Action::Seq(symbols) => {
                for symbol in symbols {
                    if *symbol == "*" {
                        out.push(p);
                    } else {
                        out.push(Ph::parse(symbol).unwrap_or_else(|| replace(p, symbol)));
                    }
                }
            }
            Action::Delete => {}
            Action::Map(pairs) => {
                let to = pairs
                    .iter()
                    .find(|&&(from, _)| from == p.symbol())
                    .map(|&(_, to)| to)
                    .expect("the target class is the map's keys");
                out.push(replace(p, to));
            }
            Action::Devoice | Action::Voice => {
                let wanted = matches!(rule.action, Action::Voice);
                match p.voicing_partner() {
                    Some(q) if p.is_voiced() != wanted => out.push(q),
                    _ => out.push(p),
                }
            }
        }
    }
}

/// The word with primary stress on its last vowel and none elsewhere.
fn final_stress(word: &[Ph]) -> Vec<Ph> {
    let last = word.iter().rposition(|p| p.is_vowel());
    word.iter()
        .enumerate()
        .map(|(i, &p)| match p.stress() {
            Some(_) if Some(i) == last => p.with_stress(1),
            Some(_) => p.with_stress(0),
            None => p,
        })
        .collect()
}

/// `p` as `symbol`, keeping a vowel's stress.
fn replace(p: Ph, symbol: &str) -> Ph {
    match p.stress() {
        Some(stress) => Ph::vowel(symbol, stress),
        None => Ph::consonant(symbol),
    }
    .unwrap_or_else(|| panic!("{p} can't become {symbol}"))
}

/// Where each vowel sits: monophthongs as a point, diphthongs as a start
/// and an end.
pub struct VowelChart {
    vowels: Vec<(&'static str, Point, Point)>,
}

impl VowelChart {
    /// General American positions on a 0–1 chart, from Hillenbrand et al.
    /// (1995), Lobanov-normalized (see `docs/accent-research.md` §2). Stressed
    /// AH is STRUT [ʌ]; ER isn't moved.
    pub fn general_american() -> Self {
        Self {
            vowels: GENERAL_AMERICAN.to_vec(),
        }
    }

    fn position(&self, symbol: &str) -> Option<(Point, Point)> {
        self.vowels
            .iter()
            .find(|(s, ..)| *s == symbol)
            .map(|&(_, a, b)| (a, b))
    }

    /// `p` moved by `moves`, snapped to the nearest vowel (keeping stress).
    pub fn moved(&self, p: Ph, moves: &[VowelMove]) -> Ph {
        let Some((start, end)) = self.position(p.symbol()) else {
            return p;
        };
        let (start, end) = moves.iter().fold((start, end), |(a, b), m| match m {
            VowelMove::ShiftOnset { .. } if a != b => (m.apply(a), b),
            _ => (m.apply(a), m.apply(b)),
        });
        let dist = |a: Point, b: Point| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        let nearest = self
            .vowels
            .iter()
            .min_by(|x, y| {
                let dx = dist(x.1, start) + dist(x.2, end);
                let dy = dist(y.1, start) + dist(y.2, end);
                dx.total_cmp(&dy)
            })
            .map(|&(symbol, ..)| symbol)
            .expect("the chart has vowels");
        replace(p, nearest)
    }
}

/// `(symbol, start, end)` as (backness, height): equal for monophthongs.
/// Measured (Hillenbrand et al. 1995, 93 adult speakers): the
/// monophthongs, and EY and OW at 20% → 80% of their duration. AE is
/// lowered from its measured 0.44 to 0.2: the speakers were from Southern
/// Michigan, where TRAP is raised (the Northern Cities Shift). AY, AW and OY
/// are placed from IPA positions within the measured frame.
const GENERAL_AMERICAN: [(&str, Point, Point); 14] = [
    ("IY", (0.00, 1.00), (0.00, 1.00)),
    ("IH", (0.22, 0.84), (0.22, 0.84)),
    ("EY", (0.16, 0.73), (0.06, 0.92)),
    ("EH", (0.39, 0.40), (0.39, 0.40)),
    ("AE", (0.26, 0.20), (0.26, 0.20)),
    ("AA", (0.72, 0.00), (0.72, 0.00)),
    ("AO", (0.92, 0.24), (0.92, 0.24)),
    ("OW", (0.98, 0.61), (1.00, 0.85)),
    ("UH", (0.87, 0.75), (0.87, 0.75)),
    ("UW", (0.95, 0.93), (0.95, 0.93)),
    ("AH", (0.79, 0.32), (0.79, 0.32)),
    ("AY", (0.50, 0.05), (0.22, 0.84)),
    ("AW", (0.50, 0.05), (0.87, 0.75)),
    ("OY", (0.92, 0.30), (0.22, 0.84)),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<Vec<Ph>> {
        s.split('|').map(|w| Ph::parse_all(w).unwrap()).collect()
    }

    fn spelled(words: &[Vec<Ph>]) -> String {
        words
            .iter()
            .map(|w| crate::phone::spell(w))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn rules_respect_their_context() {
        let non_rhotic = Deformation::new("non-rhotic").rule(
            Rule::new("r-drop", Class::is(&["R"]), Action::Delete)
                .left(Context::Class(Class::Vowel))
                .right(Context::Not(Class::Vowel)),
        );
        // "car" loses its R; "red" and "very" keep theirs.
        assert_eq!(
            spelled(&non_rhotic.apply(&words("K AA1 R|R EH1 D|V EH1 R IY0"))),
            "K AA1 | R EH1 D | V EH1 R IY0"
        );
        let devoice = Deformation::new("final devoicing").rule(
            Rule::new("devoice", Class::VoicedObstruent, Action::Devoice).right(Context::Edge),
        );
        assert_eq!(
            spelled(&devoice.apply(&words("B AE1 D|D AA1 G Z"))),
            "B AE1 T | D AA1 G S"
        );
    }

    #[test]
    fn vowel_moves_snap_to_vowels_and_keep_stress() {
        let chart = VowelChart::general_american();
        let iy1 = Ph::parse("IY1").unwrap();
        // Front ↔ back: IY ↔ UW, keeping stress.
        assert_eq!(
            chart.moved(iy1, &[VowelMove::MirrorFrontBack]).to_string(),
            "UW1"
        );
        // High ↔ low: IY → AE.
        assert_eq!(
            chart.moved(iy1, &[VowelMove::MirrorHeight]).to_string(),
            "AE1"
        );
        // No move: unchanged.
        for v in ["AA1", "AY1", "OW2", "AH1"] {
            let p = Ph::parse(v).unwrap();
            assert_eq!(chart.moved(p, &[]), p, "{v}");
        }
    }

    #[test]
    fn rates_pick_sites_deterministically() {
        let half = Deformation::new("half")
            .rule(Rule::new("th", Class::is(&["TH"]), Action::To("T")))
            .at(0.5);
        let input = words("TH IH1 NG|TH AO1 T|TH R UW1|TH AY1|TH IH1 N|TH AE1 NG K");
        let once = half.apply(&input);
        assert_eq!(once, half.apply(&input));
        let changed = once.iter().zip(&input).filter(|(a, b)| a != b).count();
        assert!(changed > 0 && changed < input.len(), "{changed}");
    }
}
