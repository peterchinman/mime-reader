use crate::error::ParseArpabetError;

// --- ConsonantPhone ---

#[derive(Debug)]
enum ConsonantPhone {
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

impl std::str::FromStr for ConsonantPhone {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CH" => Ok(ConsonantPhone::CH),
            "JH" => Ok(ConsonantPhone::JH),
            "R"  => Ok(ConsonantPhone::R),
            "W"  => Ok(ConsonantPhone::W),
            "Y"  => Ok(ConsonantPhone::Y),
            "DH" => Ok(ConsonantPhone::DH),
            "F"  => Ok(ConsonantPhone::F),
            "HH" => Ok(ConsonantPhone::HH),
            "S"  => Ok(ConsonantPhone::S),
            "SH" => Ok(ConsonantPhone::SH),
            "TH" => Ok(ConsonantPhone::TH),
            "V"  => Ok(ConsonantPhone::V),
            "Z"  => Ok(ConsonantPhone::Z),
            "ZH" => Ok(ConsonantPhone::ZH),
            "L"  => Ok(ConsonantPhone::L),
            "M"  => Ok(ConsonantPhone::M),
            "N"  => Ok(ConsonantPhone::N),
            "NG" => Ok(ConsonantPhone::NG),
            "B"  => Ok(ConsonantPhone::B),
            "D"  => Ok(ConsonantPhone::D),
            "G"  => Ok(ConsonantPhone::G),
            "K"  => Ok(ConsonantPhone::K),
            "P"  => Ok(ConsonantPhone::P),
            "T"  => Ok(ConsonantPhone::T),
            _    => Err(ParseArpabetError::UnknownPhone),
        }
    }
}

impl std::fmt::Display for ConsonantPhone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConsonantPhone::CH => "CH",
            ConsonantPhone::JH => "JH",
            ConsonantPhone::R  => "R",
            ConsonantPhone::W  => "W",
            ConsonantPhone::Y  => "Y",
            ConsonantPhone::DH => "DH",
            ConsonantPhone::F  => "F",
            ConsonantPhone::HH => "HH",
            ConsonantPhone::S  => "S",
            ConsonantPhone::SH => "SH",
            ConsonantPhone::TH => "TH",
            ConsonantPhone::V  => "V",
            ConsonantPhone::Z  => "Z",
            ConsonantPhone::ZH => "ZH",
            ConsonantPhone::L  => "L",
            ConsonantPhone::M  => "M",
            ConsonantPhone::N  => "N",
            ConsonantPhone::NG => "NG",
            ConsonantPhone::B  => "B",
            ConsonantPhone::D  => "D",
            ConsonantPhone::G  => "G",
            ConsonantPhone::K  => "K",
            ConsonantPhone::P  => "P",
            ConsonantPhone::T  => "T",
        };
        write!(f, "{}", s)
    }
}

// --- ConsonantManner ---

#[derive(Debug)]
enum ConsonantManner {
    Affricate,
    Approximant,
    Fricative,
    LateralApproximant,
    Nasal,
    Plosive,
}

// --- Consonant ---

#[derive(Debug)]
pub struct Consonant {
    phone: ConsonantPhone,
    manner: ConsonantManner,
    sibilant: bool,
    voiced: bool,
    // 0-7
    place: u8,
}

impl From<ConsonantPhone> for Consonant {
    fn from(phone: ConsonantPhone) -> Self {
        use ConsonantPhone::*;
        use ConsonantManner::*;
        let (manner, sibilant, voiced, place) = match phone {
            CH => (Affricate,          true,  false, 5),
            JH => (Affricate,          true,  true,  5),
            R  => (Approximant,        false, true,  4),
            W  => (Approximant,        false, true,  9),
            Y  => (Approximant,        false, true,  6),
            DH => (Fricative,          false, true,  3),
            F  => (Fricative,          false, false, 2),
            HH => (Fricative,          false, false, 8),
            S  => (Fricative,          true,  false, 4),
            SH => (Fricative,          true,  false, 5),
            TH => (Fricative,          false, false, 3),
            V  => (Fricative,          false, true,  2),
            Z  => (Fricative,          true,  true,  4),
            ZH => (Fricative,          true,  true,  5),
            L  => (LateralApproximant, false, true,  4),
            M  => (Nasal,              false, true,  1),
            N  => (Nasal,              false, true,  4),
            NG => (Nasal,              false, true,  7),
            B  => (Plosive,            false, true,  1),
            D  => (Plosive,            false, true,  4),
            G  => (Plosive,            false, true,  7),
            K  => (Plosive,            false, false, 7),
            P  => (Plosive,            false, false, 1),
            T  => (Plosive,            false, false, 4),
        };
        Consonant { phone, manner, sibilant, voiced, place }
    }
}

impl std::fmt::Display for Consonant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.phone)
    }
}

// --- VowelPhone ---

#[derive(Debug)]
enum VowelPhone {
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

impl std::str::FromStr for VowelPhone {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AE" => Ok(VowelPhone::AE),
            "AA" => Ok(VowelPhone::AA),
            "EH" => Ok(VowelPhone::EH),
            "AH" => Ok(VowelPhone::AH),
            "AO" => Ok(VowelPhone::AO),
            "IY" => Ok(VowelPhone::IY),
            "IH" => Ok(VowelPhone::IH),
            "UH" => Ok(VowelPhone::UH),
            "UW" => Ok(VowelPhone::UW),
            "ER" => Ok(VowelPhone::ER),
            "AW" => Ok(VowelPhone::AW),
            "AY" => Ok(VowelPhone::AY),
            "EY" => Ok(VowelPhone::EY),
            "OW" => Ok(VowelPhone::OW),
            "OY" => Ok(VowelPhone::OY),
            _    => Err(ParseArpabetError::UnknownPhone),
        }
    }
}

impl std::fmt::Display for VowelPhone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VowelPhone::AE => "AE",
            VowelPhone::AA => "AA",
            VowelPhone::EH => "EH",
            VowelPhone::AH => "AH",
            VowelPhone::AO => "AO",
            VowelPhone::IY => "IY",
            VowelPhone::IH => "IH",
            VowelPhone::UH => "UH",
            VowelPhone::UW => "UW",
            VowelPhone::ER => "ER",
            VowelPhone::AW => "AW",
            VowelPhone::AY => "AY",
            VowelPhone::EY => "EY",
            VowelPhone::OW => "OW",
            VowelPhone::OY => "OY",
        };
        write!(f, "{}", s)
    }
}

// --- Stress ---

#[derive(Debug)]
pub enum Stress {
    Unstressed,
    Primary,
    Secondary,
}

// --- Vowel ---

#[derive(Debug)]
pub struct Vowel {
    phone: VowelPhone,
    pub stress: Stress,
}

impl std::str::FromStr for Vowel {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ParseArpabetError::UnknownPhone);
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
                if s.parse::<VowelPhone>().is_ok() {
                    return Err(ParseArpabetError::MissingStress);
                }
                return Err(ParseArpabetError::UnknownPhone);
            }
        };
        let phone = vowel_str.parse::<VowelPhone>()?;
        Ok(Vowel { phone, stress })
    }
}

impl std::fmt::Display for Vowel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stress = match self.stress {
            Stress::Unstressed => '0',
            Stress::Primary    => '1',
            Stress::Secondary  => '2',
        };
        write!(f, "{}{}", self.phone, stress)
    }
}

// --- Phone ---

#[derive(Debug)]
pub enum Phone {
    Consonant(Consonant),
    Vowel(Vowel),
}

impl std::str::FromStr for Phone {
    type Err = ParseArpabetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(consonant_phone) = s.parse::<ConsonantPhone>() {
            return Ok(Phone::Consonant(Consonant::from(consonant_phone)));
        }
        Ok(Phone::Vowel(s.parse::<Vowel>()?))
    }
}

impl std::fmt::Display for Phone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phone::Consonant(c) => write!(f, "{}", c),
            Phone::Vowel(v)     => write!(f, "{}", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FromStr ---

    #[test]
    fn parse_consonant() {
        let phone: Phone = "CH".parse().unwrap();
        assert!(matches!(phone, Phone::Consonant(_)));
    }

    #[test]
    fn parse_vowel_primary_stress() {
        let phone: Phone = "AE1".parse().unwrap();
        assert!(matches!(phone, Phone::Vowel(Vowel { stress: Stress::Primary, .. })));
    }

    #[test]
    fn parse_vowel_unstressed() {
        let phone: Phone = "AE0".parse().unwrap();
        assert!(matches!(phone, Phone::Vowel(Vowel { stress: Stress::Unstressed, .. })));
    }

    #[test]
    fn parse_vowel_secondary_stress() {
        let phone: Phone = "AE2".parse().unwrap();
        assert!(matches!(phone, Phone::Vowel(Vowel { stress: Stress::Secondary, .. })));
    }

    #[test]
    fn parse_unknown_phone() {
        let err = "XY1".parse::<Phone>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::UnknownPhone));
    }

    #[test]
    fn parse_missing_stress() {
        let err = "AE".parse::<Phone>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::MissingStress));
    }

    #[test]
    fn parse_invalid_stress() {
        let err = "AE3".parse::<Phone>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::InvalidStress('3')));
    }

    #[test]
    fn parse_empty() {
        let err = "".parse::<Phone>().unwrap_err();
        assert!(matches!(err, ParseArpabetError::UnknownPhone));
    }

    // --- Display ---

    #[test]
    fn display_consonant() {
        let phone: Phone = "CH".parse().unwrap();
        assert_eq!(phone.to_string(), "CH");
    }

    #[test]
    fn display_vowel() {
        let phone: Phone = "AE1".parse().unwrap();
        assert_eq!(phone.to_string(), "AE1");
    }

    // --- Round-trip ---

    #[test]
    fn round_trip_consonants() {
        for s in ["CH", "JH", "R", "W", "Y", "DH", "F", "HH", "S", "SH",
                  "TH", "V", "Z", "ZH", "L", "M", "N", "NG", "B", "D",
                  "G", "K", "P", "T"] {
            let phone: Phone = s.parse().unwrap();
            assert_eq!(phone.to_string(), s);
        }
    }

    #[test]
    fn round_trip_vowels() {
        for s in ["AE0", "AA1", "EH2", "AH0", "AO1", "IY2", "IH0", "UH1",
                  "UW2", "ER0", "AW1", "AY2", "EY0", "OW1", "OY2"] {
            let phone: Phone = s.parse().unwrap();
            assert_eq!(phone.to_string(), s);
        }
    }
}
