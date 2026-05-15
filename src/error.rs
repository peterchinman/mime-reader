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
    InvalidChar(char),
    InvalidParenNesting,
}

impl std::fmt::Display for ParseMeterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseMeterError::InvalidChar(c) => write!(f, "invalid stress character '{}'", c),
            ParseMeterError::InvalidParenNesting => write!(f, "invalid parentheses nesting"),
        }
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
