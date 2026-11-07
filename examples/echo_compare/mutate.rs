//! Synthetic echoes: corpus lines mutated the ways real repetitions vary.

use std::collections::HashMap;

use mime_reader::{Phoneme, PronouncingDictionary};

use crate::corpus::{Label, LabeledPair};

/// xorshift64: reproducible from `--seed`, no `rand` dependency.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Never zero (xorshift's fixed point); spread small seeds out.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform in `range` (must be non-empty).
    pub fn range(&mut self, range: std::ops::Range<usize>) -> usize {
        range.start + (self.next() % (range.end - range.start) as u64) as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mutation {
    /// A donor phrase inserted inside the line (class 3).
    InsertPhrase,
    /// A span deleted from inside the line (class 3).
    DeleteSpan,
    /// The line cut short at one end (class 4).
    Truncate,
    /// Donor words appended (class 4).
    Extend,
    /// The two halves exchanged at a word boundary (class 5).
    Rotate,
    /// Two adjacent inner blocks exchanged, prefix or suffix kept (class 5).
    SwapBlocks,
    /// Three blocks reversed: X Y Z → Z Y X (class 6).
    Permute3,
    /// One word's inflection changed (class 8).
    MorphVariant,
    /// One word replaced by a homophone (class 9).
    Homophone,
    /// Words shuffled — a diagnostic, not an echo label.
    Shuffle,
}

impl Mutation {
    pub const ALL: [Self; 10] = [
        Self::InsertPhrase,
        Self::DeleteSpan,
        Self::Truncate,
        Self::Extend,
        Self::Rotate,
        Self::SwapBlocks,
        Self::Permute3,
        Self::MorphVariant,
        Self::Homophone,
        Self::Shuffle,
    ];

    pub fn class(self) -> u32 {
        match self {
            Self::InsertPhrase | Self::DeleteSpan => 3,
            Self::Truncate | Self::Extend => 4,
            Self::Rotate | Self::SwapBlocks => 5,
            Self::Permute3 => 6,
            Self::MorphVariant => 8,
            Self::Homophone => 9,
            Self::Shuffle => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::InsertPhrase => "insert_phrase",
            Self::DeleteSpan => "delete_span",
            Self::Truncate => "truncate",
            Self::Extend => "extend",
            Self::Rotate => "rotate",
            Self::SwapBlocks => "swap_blocks",
            Self::Permute3 => "permute3",
            Self::MorphVariant => "morph_variant",
            Self::Homophone => "homophone",
            Self::Shuffle => "shuffle",
        }
    }

    pub fn label(self) -> Label {
        match self {
            Self::Shuffle => Label::Unlabeled,
            _ => Label::Echo,
        }
    }
}

/// Words whose first pronunciation is shared with another plain word.
pub struct Homophones {
    by_word: HashMap<String, Vec<String>>,
}

impl Homophones {
    pub fn new<D: PronouncingDictionary>(dict: &D) -> Self {
        let mut by_sound: HashMap<&[Phoneme], Vec<&str>> = HashMap::new();
        for (word, pronunciations) in dict.entries() {
            if word.chars().all(|c| c.is_ascii_alphabetic())
                && let Some(first) = pronunciations.first()
            {
                by_sound.entry(first).or_default().push(word);
            }
        }
        let mut by_word: HashMap<String, Vec<String>> = HashMap::new();
        for words in by_sound.values().filter(|words| words.len() > 1) {
            for &word in words {
                let mut others: Vec<String> = words
                    .iter()
                    .filter(|&&other| other != word)
                    .map(|other| other.to_lowercase())
                    .collect();
                others.sort();
                by_word.insert(word.to_lowercase(), others);
            }
        }
        Self { by_word }
    }

    fn of(&self, word: &str) -> Option<&[String]> {
        self.by_word.get(word).map(Vec::as_slice)
    }
}

/// The word with surrounding punctuation split off: (prefix, bare, suffix).
fn bare(word: &str) -> (&str, &str, &str) {
    let start = word.len()
        - word
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .len();
    let end = word.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
    if start >= end {
        return (word, "", "");
    }
    (&word[..start], &word[start..end], &word[end..])
}

/// Inflectional neighbors of `word` that the dictionary knows.
fn morph_variants<D: PronouncingDictionary>(word: &str, dict: &D) -> Vec<String> {
    let lower = word.to_lowercase();
    let mut candidates = vec![
        format!("{lower}s"),
        format!("{lower}es"),
        format!("{lower}ed"),
        format!("{lower}d"),
        format!("{lower}ing"),
    ];
    for suffix in ["s", "es", "ed", "d", "ing"] {
        if let Some(stem) = lower.strip_suffix(suffix)
            && stem.len() >= 3
        {
            candidates.push(stem.to_string());
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|c| *c != lower && dict.lookup(c).is_some())
        .collect()
}

/// Applies `mutation` to `words`, borrowing material from `donor`. `None`
/// when the line is too short for it or nothing applies.
pub fn mutate<D: PronouncingDictionary>(
    mutation: Mutation,
    words: &[&str],
    donor: &[&str],
    rng: &mut Rng,
    dict: &D,
    homophones: &Homophones,
) -> Option<Vec<String>> {
    let n = words.len();
    let owned = |slice: &[&str]| slice.iter().map(|w| w.to_string()).collect::<Vec<String>>();
    let out: Vec<String> = match mutation {
        Mutation::InsertPhrase => {
            if n < 3 || donor.len() < 3 {
                return None;
            }
            let len = rng.range(3..donor.len().min(6) + 1);
            let from = rng.range(0..donor.len() - len + 1);
            let at = rng.range(1..n);
            let mut out = owned(&words[..at]);
            out.extend(owned(&donor[from..from + len]));
            out.extend(owned(&words[at..]));
            out
        }
        Mutation::DeleteSpan => {
            if n < 6 {
                return None;
            }
            let len = rng.range(2..4);
            let from = rng.range(1..n - len);
            let mut out = owned(&words[..from]);
            out.extend(owned(&words[from + len..]));
            out
        }
        Mutation::Truncate => {
            if n < 4 {
                return None;
            }
            let keep = rng.range(n.div_ceil(2)..n);
            if rng.range(0..3) == 0 {
                owned(&words[n - keep..])
            } else {
                owned(&words[..keep])
            }
        }
        Mutation::Extend => {
            if donor.len() < 2 {
                return None;
            }
            let len = rng.range(2..donor.len().min(4) + 1);
            let mut out = owned(words);
            out.extend(owned(&donor[..len]));
            out
        }
        Mutation::Rotate => {
            if n < 4 {
                return None;
            }
            let k = rng.range(1..n);
            let mut out = owned(&words[k..]);
            out.extend(owned(&words[..k]));
            out
        }
        Mutation::SwapBlocks => {
            if n < 5 {
                return None;
            }
            // [prefix][x][y][suffix] with x, y non-empty and prefix or suffix
            // non-empty (otherwise it's a rotation).
            let (p, x, y) = loop {
                let p = rng.range(0..n - 1);
                let x = rng.range(1..n - p);
                if p + x >= n {
                    continue;
                }
                let y = rng.range(1..n - p - x + 1);
                if p > 0 || p + x + y < n {
                    break (p, x, y);
                }
            };
            let mut out = owned(&words[..p]);
            out.extend(owned(&words[p + x..p + x + y]));
            out.extend(owned(&words[p..p + x]));
            out.extend(owned(&words[p + x + y..]));
            out
        }
        Mutation::Permute3 => {
            if n < 4 {
                return None;
            }
            let first = rng.range(1..n - 1);
            let second = rng.range(first + 1..n);
            let mut out = owned(&words[second..]);
            out.extend(owned(&words[first..second]));
            out.extend(owned(&words[..first]));
            out
        }
        Mutation::MorphVariant => {
            let options: Vec<(usize, String)> = words
                .iter()
                .enumerate()
                .flat_map(|(k, word)| {
                    let (pre, core, post) = bare(word);
                    morph_variants(core, dict)
                        .into_iter()
                        .map(move |v| (k, format!("{pre}{v}{post}")))
                })
                .collect();
            if options.is_empty() {
                return None;
            }
            let (k, replacement) = options[rng.range(0..options.len())].clone();
            let mut out = owned(words);
            out[k] = replacement;
            out
        }
        Mutation::Homophone => {
            let options: Vec<(usize, String)> = words
                .iter()
                .enumerate()
                .flat_map(|(k, word)| {
                    let (pre, core, post) = bare(word);
                    homophones
                        .of(&core.to_lowercase())
                        .unwrap_or_default()
                        .iter()
                        .map(move |h| (k, format!("{pre}{h}{post}")))
                        .collect::<Vec<_>>()
                })
                .collect();
            if options.is_empty() {
                return None;
            }
            let (k, replacement) = options[rng.range(0..options.len())].clone();
            let mut out = owned(words);
            out[k] = replacement;
            out
        }
        Mutation::Shuffle => {
            if n < 4 {
                return None;
            }
            let mut out = owned(words);
            for k in (1..n).rev() {
                out.swap(k, rng.range(0..k + 1));
            }
            out
        }
    };
    let original: Vec<String> = owned(words);
    (out != original && !out.is_empty()).then_some(out)
}

/// Up to `per_kind` mutated pairs of each kind, from `lines` (deduplicated,
/// at least four words), deterministic in `seed`.
pub fn mutation_pairs<D: PronouncingDictionary>(
    lines: &[String],
    per_kind: usize,
    seed: u64,
    dict: &D,
) -> Vec<LabeledPair> {
    let mut pool: Vec<&String> = lines
        .iter()
        .filter(|l| l.split_whitespace().count() >= 4)
        .collect();
    pool.sort();
    pool.dedup();
    if pool.len() < 2 {
        return Vec::new();
    }
    let homophones = Homophones::new(dict);
    let mut rng = Rng::new(seed);
    let mut pairs = Vec::new();
    for mutation in Mutation::ALL {
        let mut made = 0;
        for _attempt in 0..per_kind * 8 {
            if made == per_kind {
                break;
            }
            let line = pool[rng.range(0..pool.len())];
            let donor = pool[rng.range(0..pool.len())];
            let words: Vec<&str> = line.split_whitespace().collect();
            let donor_words: Vec<&str> = donor.split_whitespace().collect();
            if let Some(out) = mutate(mutation, &words, &donor_words, &mut rng, dict, &homophones) {
                pairs.push(LabeledPair {
                    source: "mutation".into(),
                    group: format!("{:02}:{}", mutation.class(), mutation.name()),
                    label: mutation.label(),
                    poem: None,
                    lines: None,
                    a: line.clone(),
                    b: out.join(" "),
                });
                made += 1;
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use mime_reader::CmuDictionary;

    fn dict() -> CmuDictionary {
        CmuDictionary::from_bytes(
            b"WALK  W AO1 K\nWALKS  W AO1 K S\nWALKED  W AO1 K T\nKNIGHT  N AY1 T\nNIGHT  N AY1 T\nTHE  DH AH0\nIN  IH0 N\nDARK  D AA1 R K\n",
        )
        .unwrap()
    }

    fn run(mutation: Mutation, line: &str, seed: u64) -> Option<String> {
        let dict = dict();
        let homophones = Homophones::new(&dict);
        let words: Vec<&str> = line.split_whitespace().collect();
        let donor: Vec<&str> = "green leaves fall slowly down".split_whitespace().collect();
        mutate(
            mutation,
            &words,
            &donor,
            &mut Rng::new(seed),
            &dict,
            &homophones,
        )
        .map(|w| w.join(" "))
    }

    #[test]
    fn every_mutation_changes_the_line_and_is_reproducible() {
        let line = "we walk in the dark of the night alone";
        for mutation in Mutation::ALL {
            for seed in 1..20 {
                let out = run(mutation, line, seed).unwrap_or_else(|| panic!("{mutation:?}"));
                assert_ne!(out, line, "{mutation:?}");
                assert_eq!(
                    run(mutation, line, seed),
                    Some(out),
                    "{mutation:?} reproducible"
                );
            }
        }
    }

    #[test]
    fn mutations_keep_or_reorder_the_words() {
        let line = "one two three four five six";
        let sorted = |s: &str| {
            let mut w: Vec<&str> = s.split_whitespace().collect();
            w.sort();
            w.join(" ")
        };
        for mutation in [
            Mutation::Rotate,
            Mutation::SwapBlocks,
            Mutation::Permute3,
            Mutation::Shuffle,
        ] {
            for seed in 1..30 {
                let out = run(mutation, line, seed).unwrap();
                assert_eq!(sorted(&out), sorted(line), "{mutation:?} permutes");
            }
        }
        for seed in 1..30 {
            let out = run(Mutation::Truncate, line, seed).unwrap();
            assert!(
                line.contains(&out),
                "truncation is a prefix or suffix: {out}"
            );
        }
    }

    #[test]
    fn lexical_mutations_use_the_dictionary() {
        let out = run(Mutation::MorphVariant, "we walk,", 1).unwrap();
        assert!(out == "we walks," || out == "we walked,", "{out}");
        assert_eq!(
            run(Mutation::Homophone, "the knight", 1).unwrap(),
            "the night"
        );
        assert!(run(Mutation::Homophone, "the dark", 1).is_none());
    }
}
