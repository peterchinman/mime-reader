/// The shared scale both phoneme-distance engines normalize to: a
/// [`VowelDistanceScorer`] or [`ConsonantDistanceScorer`] returns `0`
/// (identical) through `NORMALIZED_DISTANCE_MAX` (the most distant pair that
/// engine can produce under its config).
pub const NORMALIZED_DISTANCE_MAX: u32 = 1000;

pub mod analyzer;
pub mod consonant_distance;
pub mod dictionary;
pub mod error;
pub mod line;
pub mod meter;
pub mod phoneme;
pub mod poem;
pub mod rhyme_distance;
pub mod rhyme_scheme;
pub mod vowel_distance;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use analyzer::{Analyzer, Compare, RhymeMatch};
pub use consonant_distance::{
    AnalyticConsonantDistance, AnalyticConsonantDistanceConfig, ConsonantDistanceScorer,
};
pub use dictionary::{CmuDictionary, PronouncingDictionary};
pub use error::{
    CmuDictionaryError, MeterCheckError, ParseArpabetError, ParseMeterError, ParseMeterSchemeError,
    ParseRhymeError, ParseSyllableCountError, PoemEditError, RhymeCheckError, UnknownWordError,
};
pub use line::Line;
pub use meter::{
    MeterMatchResult, MeterMismatch, MeterScheme, MeterSpecification, SyllableCountSpecification,
    SyllableStress,
};
pub use phoneme::{Consonant, Phoneme, Stress, Vowel, VowelPhoneme};
pub use poem::Poem;
pub use rhyme_distance::{
    AlignmentStep, OwnedAlignmentStep, RhymeDistanceConfig, RhymeDistanceOutput,
    RhymeDistanceScorer, RhymeNormalization, RhymePairScore, TranspositionDirection,
};
pub use rhyme_scheme::{RhymeComparisonConfig, RhymeScheme};
pub use vowel_distance::{VowelDistanceScorer, VowelHexGraph, VowelHexGraphConfig};
