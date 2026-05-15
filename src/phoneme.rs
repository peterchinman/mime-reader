use crate::error::ParseArpabetError;

// --- ConsonantPhoneme ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ConsonantPhoneme {
    CH,
    JH,
    R,
    W,
    Y,
    DH,
    F,
    HH,
    S,
    SH,
    TH,
    V,
    Z,
    ZH,
    L,
    M,
    N,
    NG,
    B,
    D,
    G,
    K,
    P,
    T,
}

impl std::str::FromStr for ConsonantPhoneme {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CH" => Ok(ConsonantPhoneme::CH),
            "JH" => Ok(ConsonantPhoneme::JH),
            "R" => Ok(ConsonantPhoneme::R),
            "W" => Ok(ConsonantPhoneme::W),
            "Y" => Ok(ConsonantPhoneme::Y),
            "DH" => Ok(ConsonantPhoneme::DH),
            "F" => Ok(ConsonantPhoneme::F),
            "HH" => Ok(ConsonantPhoneme::HH),
            "S" => Ok(ConsonantPhoneme::S),
            "SH" => Ok(ConsonantPhoneme::SH),
            "TH" => Ok(ConsonantPhoneme::TH),
            "V" => Ok(ConsonantPhoneme::V),
            "Z" => Ok(ConsonantPhoneme::Z),
            "ZH" => Ok(ConsonantPhoneme::ZH),
            "L" => Ok(ConsonantPhoneme::L),
            "M" => Ok(ConsonantPhoneme::M),
            "N" => Ok(ConsonantPhoneme::N),
            "NG" => Ok(ConsonantPhoneme::NG),
            "B" => Ok(ConsonantPhoneme::B),
            "D" => Ok(ConsonantPhoneme::D),
            "G" => Ok(ConsonantPhoneme::G),
            "K" => Ok(ConsonantPhoneme::K),
            "P" => Ok(ConsonantPhoneme::P),
            "T" => Ok(ConsonantPhoneme::T),
            _ => Err(ParseArpabetError::UnknownPhoneme),
        }
    }
}

impl std::fmt::Display for ConsonantPhoneme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConsonantPhoneme::CH => "CH",
            ConsonantPhoneme::JH => "JH",
            ConsonantPhoneme::R => "R",
            ConsonantPhoneme::W => "W",
            ConsonantPhoneme::Y => "Y",
            ConsonantPhoneme::DH => "DH",
            ConsonantPhoneme::F => "F",
            ConsonantPhoneme::HH => "HH",
            ConsonantPhoneme::S => "S",
            ConsonantPhoneme::SH => "SH",
            ConsonantPhoneme::TH => "TH",
            ConsonantPhoneme::V => "V",
            ConsonantPhoneme::Z => "Z",
            ConsonantPhoneme::ZH => "ZH",
            ConsonantPhoneme::L => "L",
            ConsonantPhoneme::M => "M",
            ConsonantPhoneme::N => "N",
            ConsonantPhoneme::NG => "NG",
            ConsonantPhoneme::B => "B",
            ConsonantPhoneme::D => "D",
            ConsonantPhoneme::G => "G",
            ConsonantPhoneme::K => "K",
            ConsonantPhoneme::P => "P",
            ConsonantPhoneme::T => "T",
        };
        write!(f, "{}", s)
    }
}

// --- ConsonantManner ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ConsonantManner {
    Affricate,
    Approximant,
    Fricative,
    LateralApproximant,
    Nasal,
    Plosive,
}

// --- Consonant ---

#[derive(Debug, Clone)]
pub struct Consonant {
    pub(crate) phoneme: ConsonantPhoneme,
    pub(crate) manner: ConsonantManner,
    pub(crate) sibilant: bool,
    pub(crate) voiced: bool,
    pub(crate) place: u8,
}

impl From<ConsonantPhoneme> for Consonant {
    fn from(phoneme: ConsonantPhoneme) -> Self {
        use ConsonantManner::*;
        use ConsonantPhoneme::*;
        let (manner, sibilant, voiced, place) = match phoneme {
            CH => (Affricate, true, false, 5),
            JH => (Affricate, true, true, 5),
            R => (Approximant, false, true, 4),
            W => (Approximant, false, true, 9),
            Y => (Approximant, false, true, 6),
            DH => (Fricative, false, true, 3),
            F => (Fricative, false, false, 2),
            HH => (Fricative, false, false, 8),
            S => (Fricative, true, false, 4),
            SH => (Fricative, true, false, 5),
            TH => (Fricative, false, false, 3),
            V => (Fricative, false, true, 2),
            Z => (Fricative, true, true, 4),
            ZH => (Fricative, true, true, 5),
            L => (LateralApproximant, false, true, 4),
            M => (Nasal, false, true, 1),
            N => (Nasal, false, true, 4),
            NG => (Nasal, false, true, 7),
            B => (Plosive, false, true, 1),
            D => (Plosive, false, true, 4),
            G => (Plosive, false, true, 7),
            K => (Plosive, false, false, 7),
            P => (Plosive, false, false, 1),
            T => (Plosive, false, false, 4),
        };
        Consonant {
            phoneme,
            manner,
            sibilant,
            voiced,
            place,
        }
    }
}

impl std::fmt::Display for Consonant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.phoneme)
    }
}

// --- VowelPhone ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VowelPhoneme {
    AE,
    AA,
    EH,
    AH,
    AO,
    IY,
    IH,
    UH,
    UW,
    ER,
    AW,
    AY,
    EY,
    OW,
    OY,
}

impl std::str::FromStr for VowelPhoneme {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AE" => Ok(VowelPhoneme::AE),
            "AA" => Ok(VowelPhoneme::AA),
            "EH" => Ok(VowelPhoneme::EH),
            "AH" => Ok(VowelPhoneme::AH),
            "AO" => Ok(VowelPhoneme::AO),
            "IY" => Ok(VowelPhoneme::IY),
            "IH" => Ok(VowelPhoneme::IH),
            "UH" => Ok(VowelPhoneme::UH),
            "UW" => Ok(VowelPhoneme::UW),
            "ER" => Ok(VowelPhoneme::ER),
            "AW" => Ok(VowelPhoneme::AW),
            "AY" => Ok(VowelPhoneme::AY),
            "EY" => Ok(VowelPhoneme::EY),
            "OW" => Ok(VowelPhoneme::OW),
            "OY" => Ok(VowelPhoneme::OY),
            _ => Err(ParseArpabetError::UnknownPhoneme),
        }
    }
}

impl std::fmt::Display for VowelPhoneme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VowelPhoneme::AE => "AE",
            VowelPhoneme::AA => "AA",
            VowelPhoneme::EH => "EH",
            VowelPhoneme::AH => "AH",
            VowelPhoneme::AO => "AO",
            VowelPhoneme::IY => "IY",
            VowelPhoneme::IH => "IH",
            VowelPhoneme::UH => "UH",
            VowelPhoneme::UW => "UW",
            VowelPhoneme::ER => "ER",
            VowelPhoneme::AW => "AW",
            VowelPhoneme::AY => "AY",
            VowelPhoneme::EY => "EY",
            VowelPhoneme::OW => "OW",
            VowelPhoneme::OY => "OY",
        };
        write!(f, "{}", s)
    }
}

// --- Stress ---

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum Stress {
    Unstressed,
    Primary,
    Secondary,
}

// --- Vowel ---

#[derive(Debug, Clone)]
pub struct Vowel {
    phoneme: VowelPhoneme,
    pub stress: Stress,
}

impl std::str::FromStr for Vowel {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ParseArpabetError::UnknownPhoneme);
        }
        let (vowel_str, stress_char) = s.split_at(s.len() - 1);
        let stress = match stress_char {
            "0" => Stress::Unstressed,
            "1" => Stress::Primary,
            "2" => Stress::Secondary,
            c if c.chars().next().map_or(false, |ch| ch.is_ascii_digit()) => {
                return Err(ParseArpabetError::InvalidStress(c.chars().next().unwrap()));
            }
            _ => {
                if s.parse::<VowelPhoneme>().is_ok() {
                    return Err(ParseArpabetError::MissingStress);
                }
                return Err(ParseArpabetError::UnknownPhoneme);
            }
        };
        let phoneme = vowel_str.parse::<VowelPhoneme>()?;
        Ok(Vowel { phoneme, stress })
    }
}

impl std::fmt::Display for Vowel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stress = match self.stress {
            Stress::Unstressed => '0',
            Stress::Primary => '1',
            Stress::Secondary => '2',
        };
        write!(f, "{}{}", self.phoneme, stress)
    }
}

// --- Phoneme ---

#[derive(Debug, Clone)]
pub enum Phoneme {
    Consonant(Consonant),
    Vowel(Vowel),
}

impl std::str::FromStr for Phoneme {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(consonant_phoneme) = s.parse::<ConsonantPhoneme>() {
            return Ok(Phoneme::Consonant(Consonant::from(consonant_phoneme)));
        }
        Ok(Phoneme::Vowel(s.parse::<Vowel>()?))
    }
}

impl std::fmt::Display for Phoneme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phoneme::Consonant(c) => write!(f, "{}", c),
            Phoneme::Vowel(v) => write!(f, "{}", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FromStr ---

    #[test]
    fn parse_consonant() {
        let phoneme: Phoneme = "CH".parse().unwrap();
        assert!(matches!(phoneme, Phoneme::Consonant(_)));
    }

    #[test]
    fn parse_vowel_primary_stress() {
        let phoneme: Phoneme = "AE1".parse().unwrap();
        assert!(matches!(
            phoneme,
            Phoneme::Vowel(Vowel {
                stress: Stress::Primary,
                ..
            })
        ));
    }

    #[test]
    fn parse_vowel_unstressed() {
        let phoneme: Phoneme = "AE0".parse().unwrap();
        assert!(matches!(
            phoneme,
            Phoneme::Vowel(Vowel {
                stress: Stress::Unstressed,
                ..
            })
        ));
    }

    #[test]
    fn parse_vowel_secondary_stress() {
        let phoneme: Phoneme = "AE2".parse().unwrap();
        assert!(matches!(
            phoneme,
            Phoneme::Vowel(Vowel {
                stress: Stress::Secondary,
                ..
            })
        ));
    }

    #[test]
    fn parse_unknown_phoneme() {
        let err = "XY1".parse::<Phoneme>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::UnknownPhoneme));
    }

    #[test]
    fn parse_missing_stress() {
        let err = "AE".parse::<Phoneme>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::MissingStress));
    }

    #[test]
    fn parse_invalid_stress() {
        let err = "AE3".parse::<Phoneme>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::InvalidStress('3')));
    }

    #[test]
    fn parse_empty() {
        let err = "".parse::<Phoneme>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::UnknownPhoneme));
    }

    // --- Display ---

    #[test]
    fn display_consonant() {
        let phoneme: Phoneme = "CH".parse().unwrap();
        assert_eq!(phoneme.to_string(), "CH");
    }

    #[test]
    fn display_vowel() {
        let phoneme: Phoneme = "AE1".parse().unwrap();
        assert_eq!(phoneme.to_string(), "AE1");
    }

    // --- Round-trip ---

    #[test]
    fn round_trip_consonants() {
        for s in [
            "CH", "JH", "R", "W", "Y", "DH", "F", "HH", "S", "SH", "TH", "V", "Z", "ZH", "L", "M",
            "N", "NG", "B", "D", "G", "K", "P", "T",
        ] {
            let phoneme: Phoneme = s.parse().unwrap();
            assert_eq!(phoneme.to_string(), s);
        }
    }

    #[test]
    fn round_trip_vowels() {
        for s in [
            "AE0", "AA1", "EH2", "AH0", "AO1", "IY2", "IH0", "UH1", "UW2", "ER0", "AW1", "AY2",
            "EY0", "OW1", "OY2",
        ] {
            let phoneme: Phoneme = s.parse().unwrap();
            assert_eq!(phoneme.to_string(), s);
        }
    }
}
