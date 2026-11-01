/// The shared scale both phoneme-distance engines normalize to: a
/// [`VowelDistanceScorer`] or [`ConsonantDistanceScorer`] returns `0`
/// (identical) through `NORMALIZED_DISTANCE_MAX` (the most distant pair that
/// engine can produce under its config).
pub const NORMALIZED_DISTANCE_MAX: u32 = 1000;

pub mod analyzer;
pub mod consonant_distance;
pub mod dictionary;
pub mod error;
mod function_words;
pub mod line;
pub mod meter;
pub mod phoneme;
pub mod poem;
pub mod pronunciation;
pub mod rhyme_distance;
pub mod rhyme_scheme;
pub mod vowel_distance;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use analyzer::{
    Analyzer, Compare, ConsistentScores, ConsistentScoresOwned, PronunciationsTarget,
    RhymeIntersectionMatch, RhymeIntersectionMatchOwned, WordTarget,
};
pub use consonant_distance::{
    AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, ConsonantDistanceScorer,
};
pub use dictionary::{CmuDictionary, PronouncingDictionary, Pronunciations};
pub use error::{
    CmuDictionaryError, MeterCheckError, ParseArpabetError, ParseMeterError, ParseMeterSchemeError,
    ParseRhymeError, ParseSyllableCountError, ParseSyllableCountSchemeError, PoemEditError,
    RhymeCheckError, UnknownWordError,
};
pub use line::Line;
pub use meter::{
    Fit, Gap, MeterDeviation, MeterExceptionConfig, MeterMatchResult, MeterPosition, MeterScheme,
    MeterSpecification, ScannedStress, Scansion, SyllableCountSpecification, SyllableScansion,
    SyllableStress, WordScansion,
};
pub use phoneme::{Consonant, Phoneme, Stress, Vowel, VowelPhoneme};
pub use poem::Poem;
pub use pronunciation::{
    ConfiguredDictionary, InflectionRule, PronunciationMode, PronunciationOrigin,
    WordPronunciations,
};
pub use rhyme_distance::{
    AlignmentStep, OwnedAlignmentStep, RhymeDistanceConfig, RhymeDistanceOutput,
    RhymeDistanceScorer, RhymePairScore, RhymeScoreNormalization, TranspositionDirection,
};
pub use rhyme_scheme::{RhymeComparisonConfig, RhymeScheme};
pub use vowel_distance::{VowelDistanceScorer, VowelHexGraph, VowelHexGraphConfig};
