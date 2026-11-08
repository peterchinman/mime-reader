//! Compact phonemes: an index into the 69 ARPAbet phonemes (24 consonants,
//! 15 vowels × 3 stresses), with the articulatory features rules match on.
//! Converted to and from `mime_reader::Phoneme` through their ARPAbet
//! spelling, so this crate needs nothing but the library's public API.

use std::fmt;

pub const CONSONANTS: [&str; 24] = [
    "B", "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T",
    "TH", "V", "W", "Y", "Z", "ZH",
];
pub const VOWELS: [&str; 15] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW",
];

/// A phoneme: consonants are `0..24`, vowel `v` at stress `s` is
/// `24 + 3v + s`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ph(u8);

/// How a consonant is made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Manner {
    Stop,
    Fricative,
    Affricate,
    Nasal,
    Liquid,
    Glide,
}

impl Ph {
    pub const COUNT: usize = CONSONANTS.len() + 3 * VOWELS.len();

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn from_index(index: usize) -> Self {
        assert!(index < Self::COUNT);
        Self(index as u8)
    }

    /// Every phoneme, by index.
    pub fn all() -> impl Iterator<Item = Ph> {
        (0..Self::COUNT).map(Self::from_index)
    }

    pub fn consonant(symbol: &str) -> Option<Self> {
        CONSONANTS
            .iter()
            .position(|&c| c == symbol)
            .map(|i| Self(i as u8))
    }

    /// Vowel `symbol` (e.g. `"AE"`) at `stress` (0, 1 or 2).
    pub fn vowel(symbol: &str, stress: u8) -> Option<Self> {
        let v = VOWELS.iter().position(|&v| v == symbol)?;
        (stress < 3).then(|| Self((CONSONANTS.len() + 3 * v) as u8 + stress))
    }

    pub fn is_vowel(self) -> bool {
        self.index() >= CONSONANTS.len()
    }

    /// The vowel's index in [`VOWELS`].
    pub fn vowel_index(self) -> Option<usize> {
        self.is_vowel()
            .then(|| (self.index() - CONSONANTS.len()) / 3)
    }

    pub fn stress(self) -> Option<u8> {
        self.is_vowel()
            .then(|| ((self.index() - CONSONANTS.len()) % 3) as u8)
    }

    pub fn is_stressed(self) -> bool {
        self.stress().is_some_and(|s| s > 0)
    }

    /// The same vowel at another stress; consonants unchanged.
    pub fn with_stress(self, stress: u8) -> Self {
        match self.vowel_index() {
            Some(v) => Self::vowel(VOWELS[v], stress).expect("valid stress"),
            None => self,
        }
    }

    /// The symbol without stress: `"AE"`, `"TH"`.
    pub fn symbol(self) -> &'static str {
        match self.vowel_index() {
            Some(v) => VOWELS[v],
            None => CONSONANTS[self.index()],
        }
    }

    /// `"AE1"`, `"TH"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.as_bytes().last() {
            Some(&d @ b'0'..=b'2') => Self::vowel(&s[..s.len() - 1], d - b'0'),
            _ => Self::consonant(s),
        }
    }

    /// Parses whitespace-separated ARPAbet.
    pub fn parse_all(s: &str) -> Option<Vec<Self>> {
        s.split_whitespace().map(Self::parse).collect()
    }

    pub fn from_phoneme(phoneme: &mime_reader::Phoneme) -> Self {
        Self::parse(&phoneme.to_string()).expect("every library phoneme has an index")
    }

    pub fn to_phoneme(self) -> mime_reader::Phoneme {
        self.to_string().parse().expect("every index is a phoneme")
    }

    // ---- consonant features ----

    pub fn manner(self) -> Option<Manner> {
        use Manner::*;
        Some(match self.symbol() {
            "B" | "D" | "G" | "P" | "T" | "K" => Stop,
            "DH" | "F" | "HH" | "S" | "SH" | "TH" | "V" | "Z" | "ZH" => Fricative,
            "CH" | "JH" => Affricate,
            "M" | "N" | "NG" => Nasal,
            "L" | "R" => Liquid,
            "W" | "Y" => Glide,
            _ => return None,
        })
    }

    pub fn is_voiced(self) -> bool {
        self.is_vowel()
            || matches!(
                self.symbol(),
                "B" | "D"
                    | "G"
                    | "DH"
                    | "V"
                    | "Z"
                    | "ZH"
                    | "JH"
                    | "M"
                    | "N"
                    | "NG"
                    | "L"
                    | "R"
                    | "W"
                    | "Y"
            )
    }

    /// The voiced or voiceless partner of an obstruent, if it has one.
    pub fn voicing_partner(self) -> Option<Self> {
        let partner = match self.symbol() {
            "B" => "P",
            "P" => "B",
            "D" => "T",
            "T" => "D",
            "G" => "K",
            "K" => "G",
            "V" => "F",
            "F" => "V",
            "DH" => "TH",
            "TH" => "DH",
            "Z" => "S",
            "S" => "Z",
            "ZH" => "SH",
            "SH" => "ZH",
            "JH" => "CH",
            "CH" => "JH",
            _ => return None,
        };
        Self::consonant(partner)
    }
}

impl fmt::Display for Ph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stress() {
            Some(s) => write!(f, "{}{s}", self.symbol()),
            None => f.write_str(self.symbol()),
        }
    }
}

impl fmt::Debug for Ph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Phonemes as space-separated ARPAbet.
pub fn spell(phonemes: &[Ph]) -> String {
    phonemes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phoneme_round_trips_through_the_library() {
        for ph in Ph::all() {
            assert_eq!(Ph::parse(&ph.to_string()), Some(ph));
            assert_eq!(Ph::from_phoneme(&ph.to_phoneme()), ph);
        }
        assert_eq!(Ph::COUNT, 69);
        let ae1 = Ph::parse("AE1").unwrap();
        assert_eq!((ae1.symbol(), ae1.stress()), ("AE", Some(1)));
        assert_eq!(ae1.with_stress(0).to_string(), "AE0");
        assert!(Ph::parse("AE").is_none() && Ph::parse("XX").is_none());
    }
}
