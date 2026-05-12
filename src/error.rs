#[derive(Debug)]
pub enum ParseArpabetError {
    UnknownPhone,
    MissingStress,
    InvalidStress(char),
}

impl std::fmt::Display for ParseArpabetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseArpabetError::UnknownPhone     => write!(f, "unknown ARPAbet phone"),
            ParseArpabetError::MissingStress    => write!(f, "vowel is missing stress digit"),
            ParseArpabetError::InvalidStress(c) => write!(f, "invalid stress digit '{}'", c),
        }
    }
}

#[derive(Debug)]
pub enum DictionaryError {
    Io(std::io::Error),
    MalformedLine(String),
    InvalidPhone(ParseArpabetError),
    UnknownWord(String),
}

impl std::fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictionaryError::Io(e)             => write!(f, "IO error: {}", e),
            DictionaryError::MalformedLine(l)  => write!(f, "malformed dictionary line: {:?}", l),
            DictionaryError::InvalidPhone(e)   => write!(f, "invalid phone: {}", e),
            DictionaryError::UnknownWord(w)    => write!(f, "unknown word: {:?}", w),
        }
    }
}
