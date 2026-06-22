use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseArpabetError {
    #[error("unknown ARPAbet phoneme: {0}")]
    UnknownPhoneme(String),
    #[error("vowel phoneme '{0}' is missing stress digit")]
    MissingStress(String),
    #[error("invalid stress digit '{stress}' in phoneme '{phoneme}'")]
    InvalidStress { phoneme: String, stress: char },
}

#[derive(Debug, Error)]
pub enum DictionaryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed dictionary line: {0:?}")]
    MalformedLine(String),
    #[error("invalid phoneme: {0}")]
    InvalidPhoneme(#[from] ParseArpabetError),
}

#[derive(Debug, Error, PartialEq)]
#[error("unknown word: {word}")]
pub struct UnknownWordError {
    pub word: String,
}

#[derive(Debug, Error)]
pub enum ParseMeterError {
    #[error("invalid stress character '{c}' at column {}", col + 1)]
    InvalidChar { c: char, col: usize },
    #[error("invalid parentheses nesting at column {}", col + 1)]
    InvalidParenNesting { col: usize },
}

#[derive(Debug, Error)]
#[error("line {}: {source}", line + 1)]
pub struct ParseMeterSchemeError {
    pub line: usize,
    pub source: ParseMeterError,
}

#[derive(Debug, Error)]
pub enum ParseSyllableCountError {
    #[error("expected a number or 'min-max' range")]
    InvalidNumber,
    #[error("min must be <= max")]
    InvalidRange,
}

#[derive(Debug, Error)]
pub enum ParseRhymeError {
    #[error("invalid rhyme character '{c}' at column {}", col + 1)]
    InvalidChar { c: char, col: usize },
}

#[derive(Debug, Error, PartialEq)]
pub enum RhymeCheckError {
    #[error("target line index {target_index} is out of bounds for {line_count} lines")]
    TargetLineOutOfBounds {
        target_index: usize,
        line_count: usize,
    },
    #[error(transparent)]
    UnknownWord(#[from] UnknownWordError),
    #[error("empty line")]
    EmptyLine,
    #[error("not enough syllables")]
    NotEnoughSyllablesInTarget,
}

#[derive(Debug, Error, PartialEq)]
pub enum PoemEditError {
    #[error("line index {index} is out of bounds for {line_count} lines")]
    LineIndexOutOfBounds { index: usize, line_count: usize },
}

#[derive(Debug, Error, PartialEq)]
pub enum MeterCheckError {
    #[error("target line index {target_index} is out of bounds for {line_count} lines")]
    TargetLineOutOfBounds {
        target_index: usize,
        line_count: usize,
    },
    #[error(transparent)]
    UnknownWord(#[from] UnknownWordError),
}
