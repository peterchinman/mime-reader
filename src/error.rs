#[derive(Debug)]
pub enum ParseArpabetError {
    UnknownPhoneme,
    MissingStress,
    InvalidStress(char),
}

impl std::fmt::Display for ParseArpabetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseArpabetError::UnknownPhoneme => write!(f, "unknown ARPAbet phoneme"),
            ParseArpabetError::MissingStress => write!(f, "vowel is missing stress digit"),
            ParseArpabetError::InvalidStress(c) => write!(f, "invalid stress digit '{}'", c),
        }
    }
}

#[derive(Debug)]
pub enum DictionaryError {
    Io(std::io::Error),
    MalformedLine(String),
    InvalidPhoneme(ParseArpabetError),
}

impl std::fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictionaryError::Io(e) => write!(f, "IO error: {}", e),
            DictionaryError::MalformedLine(l) => write!(f, "malformed dictionary line: {:?}", l),
            DictionaryError::InvalidPhoneme(e) => write!(f, "invalid phoneme: {}", e),
        }
    }
}

#[derive(Debug)]
pub enum ParseMeterError {
    InvalidChar { c: char, col: usize },
    InvalidParenNesting { col: usize },
}

impl std::fmt::Display for ParseMeterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseMeterError::InvalidChar { c, col } => {
                write!(f, "invalid stress character '{c}' at column {col}")
            }
            ParseMeterError::InvalidParenNesting { col } => {
                write!(f, "invalid parentheses nesting at column {col}")
            }
        }
    }
}

#[derive(Debug)]
pub struct ParseMeterSchemeError {
    pub line: usize,
    pub source: ParseMeterError,
}

impl std::fmt::Display for ParseMeterSchemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line + 1, self.source) // +1 for human-readable
    }
}

#[derive(Debug)]
pub enum ParseSyllableCountError {
    InvalidNumber,
    InvalidRange,
}

impl std::fmt::Display for ParseSyllableCountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseSyllableCountError::InvalidNumber => {
                write!(f, "expected a number or 'min-max' range")
            }
            ParseSyllableCountError::InvalidRange => write!(f, "min must be <= max"),
        }
    }
}

#[derive(Debug)]
pub enum MeterMatchError {
    // TODO we probably want errors for??? too long, too short, first incorrect word?, unrecognized word?
    FailedMatch,
}

impl std::fmt::Display for MeterMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeterMatchError::FailedMatch => write!(f, "failed to match meter"),
        }
    }
}

#[derive(Debug)]
pub enum ParseRhymeError {
    InvalidChar(char),
}

impl std::fmt::Display for ParseRhymeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseRhymeError::InvalidChar(c) => write!(f, "invalid rhyme character '{}'", c),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum RhymeCheckError {
    TargetLineOutOfBounds {
        target_index: usize,
        line_count: usize,
    },
    UnableToDetermineDistance {
        target_index: usize,
        leader_line: usize,
    },
}

impl std::fmt::Display for RhymeCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RhymeCheckError::TargetLineOutOfBounds {
                target_index,
                line_count,
            } => write!(
                f,
                "target line index {target_index} is out of bounds for {line_count} lines"
            ),
            RhymeCheckError::UnableToDetermineDistance {
                target_index,
                leader_line,
            } => write!(
                f,
                "unable to determine rhyme distance between leader line {leader_line} and target line {target_index}"
            ),
        }
    }
}
