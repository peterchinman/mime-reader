use std::{collections::HashSet, ops::Range};

use crate::{
    PronouncingDictionary, PronunciationOrigin, Stress, WordPronunciations, phoneme::Phoneme,
};

pub struct Pronunciation {
    pub phonemes: Box<[Phoneme]>,
    pub stress_pattern: Vec<Stress>,
}

pub enum WordData {
    Unknown,
    Known {
        pronunciations: Vec<Pronunciation>,
        origin: PronunciationOrigin,
    },
}

impl WordData {
    /// Returns the stress patterns of the word, if known.
    pub fn stress_patterns(&self) -> Option<HashSet<&[Stress]>> {
        match self {
            WordData::Known { pronunciations, .. } => Some(
                pronunciations
                    .iter()
                    .map(|p| p.stress_pattern.as_slice())
                    .collect(),
            ),
            WordData::Unknown => None,
        }
    }

    /// Where this word's pronunciations came from, or `None` when it is unknown.
    pub fn origin(&self) -> Option<&PronunciationOrigin> {
        match self {
            WordData::Known { origin, .. } => Some(origin),
            WordData::Unknown => None,
        }
    }
}

pub struct WordEntry {
    pub word: String,
    /// Where `word` sits in its line's [`Line::text`], as a byte range.
    /// Punctuation ignored when reading the word lies outside it.
    pub span: Range<usize>,
    pub data: WordData,
}

impl From<WordPronunciations<'_>> for WordData {
    fn from(word_pronunciations: WordPronunciations<'_>) -> Self {
        let pronunciations = word_pronunciations
            .pronunciations
            .into_owned()
            .into_iter()
            .map(|phonemes| {
                let stress_pattern = phonemes
                    .iter()
                    .filter_map(|phoneme| match phoneme {
                        Phoneme::Vowel(vowel) => Some(vowel.stress),
                        Phoneme::Consonant(_) => None,
                    })
                    .collect();
                Pronunciation {
                    phonemes,
                    stress_pattern,
                }
            })
            .collect();
        WordData::Known {
            pronunciations,
            origin: word_pronunciations.origin,
        }
    }
}

impl WordEntry {
    /// The word at `span` in `text`, the line it is read from.
    fn new(text: &str, span: Range<usize>, data: WordData) -> Self {
        Self {
            word: text[span.clone()].to_string(),
            span,
            data,
        }
    }
}

/// Reads the whitespace-separated token at `token` in `text` into words,
/// ignoring the punctuation around a word unless the dictionary spells the
/// word with it. Tries the token as written (*Jr.*, *'tis*, *well-known*),
/// then as the word with the apostrophes touching it and a period after them,
/// for an abbreviation (*U.S.,* -> *U.S.*), then without that period (*'tis,*
/// -> *'tis*), then as the bare word, since those apostrophes may be
/// quotation marks (*'Hello,'* -> *Hello*). If none of those is known, a token
/// joined by hyphens or dashes is read as its parts (*sun-kissed* -> *sun*,
/// *kissed*). An unknown word keeps an apostrophe on one side (*xyzzin'*) but
/// not on both (*'xyzzy'*). A token with no letters, digits or `&`, such as a
/// dash, adds no words.
fn read_token<D: PronouncingDictionary + ?Sized>(
    text: &str,
    token: Range<usize>,
    dict: &D,
    words: &mut Vec<WordEntry>,
) {
    // `start..end` is the bare word: the token less everything around it
    // that isn't a letter, digit or `&`, apostrophes included.
    let is_punctuation = |character: char| !character.is_alphanumeric() && character != '&';
    let written = &text[token.clone()];
    let start = token.end - written.trim_start_matches(is_punctuation).len();
    let end = token.start + written.trim_end_matches(is_punctuation).len();
    if start >= end {
        return;
    }
    let unquoted = start..end;

    // Apostrophes touching the bare word may belong to it ('tis, dancin'),
    // and so may a period after them (U.S., Jr.).
    let unpunctuated_start = token.start
        + text[token.start..start]
            .trim_end_matches(is_apostrophe)
            .len();
    let unpunctuated_end = token.end - text[end..token.end].trim_start_matches(is_apostrophe).len();
    let unpunctuated = unpunctuated_start..unpunctuated_end;
    let period = usize::from(text[unpunctuated_end..token.end].starts_with('.'));
    let abbreviation = unpunctuated_start..unpunctuated_end + period;

    // Each spelling contains the next, so any repeats are adjacent.
    let mut spellings = vec![token, abbreviation, unpunctuated.clone(), unquoted.clone()];
    spellings.dedup();

    let known = spellings.into_iter().find_map(|spelling| {
        let word_pronunciations = dict.pronunciations_for(&text[spelling.clone()])?;
        Some((spelling, word_pronunciations))
    });
    if let Some((spelling, word_pronunciations)) = known {
        words.push(WordEntry::new(text, spelling, word_pronunciations.into()));
        return;
    }

    let quoted = unpunctuated_start < start && unpunctuated_end > end;
    let unknown = if quoted { unquoted } else { unpunctuated };
    if text[unknown.clone()].contains(is_dash) {
        for part in split_range(text, unknown, is_dash) {
            read_token(text, part, dict, words);
        }
    } else {
        words.push(WordEntry::new(text, unknown, WordData::Unknown));
    }
}

/// Splits `range` of `text` at each character matching `separator`, giving
/// the ranges between them, empty ones included.
fn split_range(
    text: &str,
    range: Range<usize>,
    separator: fn(char) -> bool,
) -> impl Iterator<Item = Range<usize>> + '_ {
    let offset = range.start;
    let separators = text[range.clone()]
        .match_indices(separator)
        .map(move |(index, matched)| offset + index..offset + index + matched.len());
    let mut piece_start = range.start;
    separators
        .chain(std::iter::once(range.end..range.end))
        .map(move |separator| {
            let piece = piece_start..separator.start;
            piece_start = separator.end;
            piece
        })
}

/// Straight and typographic apostrophes, which can belong to a word ('tis,
/// kettles') where other punctuation cannot. Editors often turn the
/// apostrophe of a leading elision into an opening quote (‘tis).
fn is_apostrophe(character: char) -> bool {
    matches!(character, '\'' | '’' | '‘')
}

/// Hyphens, which join compounds, and the dashes that separate words.
fn is_dash(character: char) -> bool {
    matches!(character, '-' | '‐' | '‑' | '–' | '—')
}

pub struct Line {
    /// The text the line was read from. Each word's span indexes it.
    pub text: String,
    pub words: Vec<WordEntry>,
}

impl Line {
    /// Splits `s` into words at whitespace. Punctuation around a word is
    /// ignored unless the dictionary spells the word with it, a hyphenated
    /// compound the dictionary doesn't list is read as its parts, and a token
    /// with no letters, digits or `&`, such as a dash, is not a word. Word
    /// indices therefore count words, not whitespace-separated tokens; each
    /// word's span locates it in [`Self::text`].
    pub fn new<D: PronouncingDictionary + ?Sized>(s: &str, dict: &D) -> Self {
        let mut words = Vec::new();
        for token in split_range(s, 0..s.len(), char::is_whitespace) {
            read_token(s, token, dict, &mut words);
        }
        Self {
            text: s.to_string(),
            words,
        }
    }
    pub fn unknown_words(&self) -> HashSet<&str> {
        self.words
            .iter()
            .filter_map(|word| match &word.data {
                WordData::Known { .. } => None,
                WordData::Unknown => Some(word.word.as_str()),
            })
            .collect()
    }

    /// The distinct total syllable counts across the line's pronunciation
    /// combinations, first-seen order — a one-word line reproduces the
    /// word's per-pronunciation counts. `None` if any word is unknown or
    /// the line is empty.
    pub fn syllable_counts(&self) -> Option<Vec<usize>> {
        if self.words.is_empty() {
            return None;
        }
        let mut totals: Vec<usize> = vec![0];
        for entry in &self.words {
            let WordData::Known { pronunciations, .. } = &entry.data else {
                return None;
            };
            let mut next: Vec<usize> = Vec::new();
            for total in &totals {
                for pronunciation in pronunciations {
                    let sum = total + pronunciation.stress_pattern.len();
                    if !next.contains(&sum) {
                        next.push(sum);
                    }
                }
            }
            totals = next;
        }
        Some(totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use crate::dictionary::Pronunciations;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    // Owns its one entry: the trait lends pronunciations out, so a
    // dictionary can't fabricate them per lookup.
    struct TestDictionary {
        gato: Pronunciations,
    }

    impl TestDictionary {
        fn new() -> Self {
            TestDictionary {
                gato: vec![
                    ["G", "AA1", "T", "OW0"]
                        .into_iter()
                        .map(|phoneme| phoneme.parse().unwrap())
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ],
            }
        }
    }

    impl PronouncingDictionary for TestDictionary {
        fn lookup(&self, word: &str) -> Option<&[Box<[Phoneme]>]> {
            (word == "gato").then_some(self.gato.as_slice())
        }
        fn entries(&self) -> impl Iterator<Item = (&str, &[Box<[Phoneme]>])> {
            std::iter::empty()
        }
    }

    // --- Line ---

    #[test]
    fn blank_line_has_no_words() {
        let line = Line::new("", dict());
        assert!(line.words.is_empty());
    }

    #[test]
    fn known_words_have_pronunciations() {
        let line = Line::new("hello world", dict());
        assert_eq!(line.words.len(), 2);
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Known { .. }));
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Dictionary { normalized: false })
        );
    }

    #[test]
    fn accepts_custom_pronouncing_dictionary() {
        let line = Line::new("gato missing", &TestDictionary::new());

        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Unknown));
    }

    #[test]
    fn unknown_word_returns_unknown() {
        let line = Line::new("hello xyzzy", dict());
        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert!(matches!(line.words[1].data, WordData::Unknown));
    }

    #[test]
    fn derived_words_require_opt_in_and_keep_their_base_syllable_count() {
        let strict = Line::new("kettle’s", dict());
        assert_eq!(strict.unknown_words(), HashSet::from(["kettle’s"]));
        assert_eq!(strict.syllable_counts(), None);

        let permissive = dict().with_pronunciation_mode(crate::PronunciationMode::AllowDerived);
        let line = Line::new("kettle’s", &permissive);

        assert!(matches!(line.words[0].data, WordData::Known { .. }));
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Derived {
                dictionary_base: "kettle".to_string(),
                rules: vec![crate::InflectionRule::ApostropheS],
            })
        );
        assert_eq!(line.syllable_counts(), Some(vec![2]));
    }

    #[test]
    fn word_with_multiple_pronunciations() {
        // "hello" has two entries in CMUdict: HELLO and HELLO(1)
        let line = Line::new("hello", dict());
        let WordData::Known {
            ref pronunciations, ..
        } = line.words[0].data
        else {
            panic!()
        };
        assert_eq!(pronunciations.len(), 2);
    }

    #[test]
    fn stress_patterns_deduplicated() {
        // both pronunciations of "hello" have the same stress pattern, so HashSet should have 1 entry
        let line = Line::new("hello", dict());
        assert_eq!(line.words[0].data.stress_patterns().unwrap().len(), 1);
    }

    #[test]
    fn original_words_are_preserved() {
        let line = Line::new("Hello World", dict());
        assert_eq!(line.words[0].word, "Hello");
        assert_eq!(line.words[1].word, "World");
    }

    #[test]
    fn syllable_counts_sum_across_words() {
        // ice (1) + cream (1) = 2.
        assert_eq!(
            Line::new("ice cream", dict()).syllable_counts(),
            Some(vec![2])
        );
        // Words with multiple pronunciations sum per combination: fire (2 or
        // 1) + truck (1) gives distinct totals in first-seen order.
        assert_eq!(
            Line::new("fire truck", dict()).syllable_counts(),
            Some(vec![3, 2])
        );
        // Unknown words and empty lines have no counts.
        assert_eq!(Line::new("qzxqzx", dict()).syllable_counts(), None);
        assert_eq!(Line::new("", dict()).syllable_counts(), None);
    }

    // --- Punctuation ---

    fn words(line: &Line) -> Vec<&str> {
        line.words.iter().map(|word| word.word.as_str()).collect()
    }

    #[test]
    fn surrounding_punctuation_is_ignored() {
        let line = Line::new("“Hello,” she said; (light.)", dict());
        assert_eq!(words(&line), ["Hello", "she", "said", "light"]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn tokens_without_letters_or_digits_are_not_words() {
        let line = Line::new("cat — dog … -- ' '?' '...'", dict());
        assert_eq!(words(&line), ["cat", "dog"]);
        assert_eq!(line.syllable_counts(), Some(vec![2]));
        assert!(Line::new("— …", dict()).words.is_empty());
    }

    #[test]
    fn ampersand_reads_as_and() {
        let line = Line::new("cats & dogs, (&)", dict());
        assert_eq!(words(&line), ["cats", "&", "dogs", "&"]);
        assert_eq!(line.syllable_counts(), Some(vec![4]));
        assert_eq!(
            line.words[1].data.origin(),
            Some(&PronunciationOrigin::Dictionary { normalized: true })
        );
    }

    #[test]
    fn unlisted_hyphenated_compounds_are_read_as_their_parts() {
        // CMUdict lists okey-dokey, so it stays one word.
        let line = Line::new("okey-dokey, sun-kissed ‘storm-tossed’", dict());
        assert_eq!(
            words(&line),
            ["okey-dokey", "sun", "kissed", "storm", "tossed"]
        );
        assert!(line.unknown_words().is_empty());

        // An unknown part is reported on its own.
        let line = Line::new("sun-xyzzy", dict());
        assert_eq!(words(&line), ["sun", "xyzzy"]);
        assert_eq!(line.unknown_words(), HashSet::from(["xyzzy"]));
    }

    #[test]
    fn dashes_inside_a_token_separate_words() {
        for text in ["light—and dark", "light–and dark", "light--and dark"] {
            assert_eq!(words(&Line::new(text, dict())), ["light", "and", "dark"]);
        }
    }

    #[test]
    fn spans_locate_words_without_their_punctuation() {
        let line = Line::new("“Hello,” sun-kissed & light—", dict());
        let spans: Vec<_> = line.words.iter().map(|word| word.span.clone()).collect();
        // “ is three bytes, so "Hello" starts at byte 3.
        assert_eq!(spans, [3..8, 13..16, 17..23, 24..25, 26..31]);
        for word in &line.words {
            assert_eq!(&line.text[word.span.clone()], word.word);
        }

        // A no-break space is two bytes and an ideographic space three.
        let line = Line::new("cat\u{a0}\u{3000}dog", dict());
        let spans: Vec<_> = line.words.iter().map(|word| word.span.clone()).collect();
        assert_eq!(spans, [0..3, 8..11]);
    }

    #[test]
    fn dictionary_spellings_keep_their_punctuation() {
        let line = Line::new("Jr. U.S. 'tis, dancin'!", dict());
        assert_eq!(words(&line), ["Jr.", "U.S.", "'tis", "dancin'"]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn edge_apostrophes_are_dropped_only_when_the_word_needs_it() {
        let line = Line::new("'hello' ‘world’ xyzzin',", dict());
        assert_eq!(words(&line), ["hello", "world", "xyzzin'"]);
        assert_eq!(line.unknown_words(), HashSet::from(["xyzzin'"]));
    }

    #[test]
    fn a_period_ending_a_sentence_is_not_an_abbreviation() {
        // CMUdict's MASS. and IN. include Massachusetts and inch.
        assert_eq!(
            Line::new("a darkened mass.", dict()).syllable_counts(),
            Some(vec![4])
        );
        let line = Line::new("let me in.", dict());
        let WordData::Known { pronunciations, .. } = &line.words[2].data else {
            panic!("in should be known");
        };
        let phonemes = pronunciations
            .iter()
            .map(|pronunciation| &pronunciation.phonemes);
        assert!(phonemes.eq(dict().lookup("in").unwrap()));
    }

    #[test]
    fn abbreviations_keep_their_period_before_other_punctuation() {
        let line = Line::new("U.S., Jr.; (St.)", dict());
        assert_eq!(words(&line), ["U.S.", "Jr.", "St."]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn quotation_marks_outside_punctuation_are_dropped() {
        let line = Line::new("'Hello,' ‘Hello!’ 'storm-tossed,'", dict());
        assert_eq!(words(&line), ["Hello", "Hello", "storm", "tossed"]);
        assert!(line.unknown_words().is_empty());
    }

    #[test]
    fn an_opening_quote_can_mark_an_elision() {
        // 'EM is AH0 M, where EM is EH1 M.
        let line = Line::new("‘em", dict());
        assert_eq!(words(&line), ["‘em"]);
        let WordData::Known { pronunciations, .. } = &line.words[0].data else {
            panic!("‘em should be known");
        };
        assert_eq!(
            *pronunciations[0].phonemes,
            *dict().lookup("'em").unwrap()[0]
        );
    }

    #[test]
    fn unknown_words_drop_apostrophes_on_both_sides() {
        let line = Line::new("'xyzzy' ‘xyzzy’ 'xyzzy xyzzy'", dict());
        assert_eq!(words(&line), ["xyzzy", "xyzzy", "'xyzzy", "xyzzy'"]);
    }

    #[test]
    fn plural_possessives_keep_their_apostrophe_when_derivable() {
        // Without derivation, a plural possessive reads as its plural.
        let strict = Line::new("kettles’,", dict());
        assert_eq!(words(&strict), ["kettles"]);

        let permissive = dict().with_pronunciation_mode(crate::PronunciationMode::AllowDerived);
        let line = Line::new("kettles’,", &permissive);
        assert_eq!(words(&line), ["kettles’"]);
        assert_eq!(
            line.words[0].data.origin(),
            Some(&PronunciationOrigin::Derived {
                dictionary_base: "kettles".to_string(),
                rules: vec![crate::InflectionRule::SApostrophe],
            })
        );
    }
}
