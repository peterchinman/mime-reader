//! Named deformations that recur across accents: the consonant knobs, and
//! a parser for vowel-space knobs (`pull:1,1,0.4`, `rotate:-30`, …).

use crate::deform::{Action, Class, Context, Deformation, LAX, Rule, TENSE, VowelMove};

/// A consonant knob by name (see [`CONSONANT_KNOBS`]).
pub fn consonant(name: &str) -> Option<Deformation> {
    let rule = |r: Rule| Deformation::new(name).rule(r);
    let vowel = || Context::Class(Class::Vowel);
    Some(match name {
        // "thing" → "ting", "this" → "dis".
        "th-stopping" => rule(Rule::new("TH → T", Class::is(&["TH"]), Action::To("T")))
            .rule(Rule::new("DH → D", Class::is(&["DH"]), Action::To("D"))),
        // "think" → "fink", "brother" → "bruvver" ("this" stays "dis").
        "th-fronting" => rule(Rule::new("TH → F", Class::is(&["TH"]), Action::To("F")))
            .rule(
                Rule::new("DH → V", Class::is(&["DH"]), Action::To("V"))
                    .left(Context::Class(Class::Vowel)),
            )
            .rule(Rule::new("DH → D", Class::is(&["DH"]), Action::To("D")).left(Context::Edge)),
        // RP-01 (accent-research.md): R drops unless a vowel follows, with
        // RP's outputs: NEAR IH R → IH AH0, CURE UH R → AO, and the rest just
        // lose the R ("car" → "cah"); NURSE ER → AH, the nearest ARPAbet
        // has to [ɜː] ("bird" → "bud").
        "non-rhotic" => rule(
            Rule::new("CURE → AO", Class::is(&["UH"]), Action::To("AO"))
                .right(Context::Class(Class::is(&["R"]))),
        )
        .rule(
            Rule::new(
                "NEAR R → schwa",
                Class::is(&["R"]),
                Action::Seq(vec!["AH0"]),
            )
            .left(Context::Class(Class::is(&["IH", "IY"])))
            .right(Context::Not(Class::Vowel)),
        )
        .rule(
            Rule::new(
                "R → ∅ unless a vowel follows",
                Class::is(&["R"]),
                Action::Delete,
            )
            .left(vowel())
            .right(Context::Not(Class::Vowel)),
        )
        .rule(Rule::new("ER → AH", Class::is(&["ER"]), Action::To("AH"))),
        // "house" → "ouse".
        "h-dropping" => {
            rule(Rule::new("HH → ∅", Class::is(&["HH"]), Action::Delete).left(Context::Edge))
        }
        // "bad" → "bat", "dogs" → "docks".
        "final-devoicing" => rule(
            Rule::new(
                "devoice at the word's end",
                Class::VoicedObstruent,
                Action::Devoice,
            )
            .right(Context::Edge),
        ),
        // CKN-06: coda L becomes a back vowel after front and central
        // vowels ("milk" → M IH1 UH0 K) and disappears after back rounded
        // ones ("ball" → "baw"). ARPAbet W can't end a syllable.
        "l-vocalization" => rule(
            Rule::new(
                "L → ∅ after a back rounded vowel",
                Class::is(&["L"]),
                Action::Delete,
            )
            .left(Context::Class(Class::is(&["AO", "OW", "UW", "UH"])))
            .right(Context::Not(Class::Vowel)),
        )
        .rule(
            Rule::new(
                "L → UH0 unless a vowel follows",
                Class::is(&["L"]),
                Action::Seq(vec!["UH0"]),
            )
            .left(vowel())
            .right(Context::Not(Class::Vowel)),
        ),
        // "going" → "goin'".
        "g-dropping" => rule(
            Rule::new("-ing → -in", Class::is(&["NG"]), Action::To("N"))
                .left(Context::Class(Class::UnstressedVowel))
                .right(Context::Edge),
        ),
        // "butter" → "bu'er", "got" → "go'": the glottal stop, which
        // ARPAbet lacks, heard as nothing.
        "t-glottaling" => rule(
            Rule::new("T → ∅ between vowels", Class::is(&["T"]), Action::Delete)
                .left(vowel())
                .right(Context::Class(Class::UnstressedVowel)),
        )
        .rule(
            Rule::new("T → ∅ at the end", Class::is(&["T"]), Action::Delete)
                .left(vowel())
                .right(Context::Edge),
        ),
        // "west" → "wes", "hand" → "han".
        "cluster-reduction" => rule(
            Rule::new(
                "final T/D after a consonant → ∅",
                Class::is(&["T", "D"]),
                Action::Delete,
            )
            .left(Context::Class(Class::Consonant))
            .right(Context::Edge),
        ),
        // "tune" → "toon", "new" → "noo".
        "yod-dropping" => rule(
            Rule::new(
                "Y → ∅ before UW after a coronal",
                Class::is(&["Y"]),
                Action::Delete,
            )
            .left(Context::Class(Class::is(&[
                "T", "D", "N", "S", "Z", "L", "TH",
            ])))
            .right(Context::Class(Class::is(&["UW"]))),
        ),
        "w-to-v" => rule(Rule::new("W → V", Class::is(&["W"]), Action::To("V"))),
        "v-to-w" => rule(Rule::new("V → W", Class::is(&["V"]), Action::To("W"))),
        "v-to-b" => rule(Rule::new("V → B", Class::is(&["V"]), Action::To("B"))),
        "r-to-l" => rule(Rule::new("R → L", Class::is(&["R"]), Action::To("L"))),
        "l-to-r" => rule(Rule::new("L → R", Class::is(&["L"]), Action::To("R"))),
        // "film" → "fillum".
        "epenthesis" => rule(
            Rule::new(
                "a schwa between L and M",
                Class::is(&["L"]),
                Action::Seq(vec!["*", "AH0"]),
            )
            .right(Context::Class(Class::is(&["M"]))),
        ),
        // Voiced stops between vowels become fricatives (Spanish):
        // "baby" → "bavy", "lady" → "lathy".
        "lenition" => rule(
            Rule::new("B → V between vowels", Class::is(&["B"]), Action::To("V"))
                .left(vowel())
                .right(vowel()),
        )
        .rule(
            Rule::new("D → DH between vowels", Class::is(&["D"]), Action::To("DH"))
                .left(vowel())
                .right(vowel()),
        ),
        _ => return None,
    })
}

pub const CONSONANT_KNOBS: [&str; 17] = [
    "th-stopping",
    "th-fronting",
    "non-rhotic",
    "h-dropping",
    "final-devoicing",
    "l-vocalization",
    "g-dropping",
    "t-glottaling",
    "cluster-reduction",
    "yod-dropping",
    "w-to-v",
    "v-to-w",
    "v-to-b",
    "r-to-l",
    "l-to-r",
    "epenthesis",
    "lenition",
];

/// Parses a vowel knob with an optional mask: `MOVE[@MASK]`, where MASK is
/// `tense`, `lax`, `front`, `back` or `diphthongs`; or one of Labov's
/// (1994) chain-shift tendencies, each with an amount: `tense-up:A` ("long
/// vowels rise"), `lax-down:A` ("short vowels fall"), `onsets-down:A` ("the
/// nuclei of upgliding diphthongs fall"), `back-to-front:A` ("back vowels
/// move to the front").
pub fn vowel_knob(spec: &str) -> Result<(Vec<VowelMove>, Option<Class>), String> {
    let (kind, amount) = spec.split_once(':').unwrap_or((spec, ""));
    let a = || amount.parse::<f32>().map_err(|e| format!("{spec}: {e}"));
    match kind {
        "tense-up" => {
            return Ok((
                vec![VowelMove::Shift { dx: 0.0, dy: a()? }],
                Some(Class::Is(TENSE.to_vec())),
            ));
        }
        "lax-down" => {
            return Ok((
                vec![VowelMove::Shift { dx: 0.0, dy: -a()? }],
                Some(Class::Is(LAX.to_vec())),
            ));
        }
        "onsets-down" => {
            return Ok((
                vec![VowelMove::ShiftOnset { dx: 0.0, dy: -a()? }],
                Some(Class::is(&DIPHTHONGS)),
            ));
        }
        "back-to-front" => {
            return Ok((
                vec![VowelMove::Shift { dx: -a()?, dy: 0.0 }],
                Some(Class::is(&BACK)),
            ));
        }
        _ => {}
    }
    let (spec, mask) = spec.split_once('@').unwrap_or((spec, ""));
    let only = match mask {
        "" => None,
        "tense" => Some(Class::Is(TENSE.to_vec())),
        "lax" => Some(Class::Is(LAX.to_vec())),
        "front" => Some(Class::is(&FRONT)),
        "back" => Some(Class::is(&BACK)),
        "diphthongs" => Some(Class::is(&DIPHTHONGS)),
        other => return Err(format!("unknown vowel mask @{other}")),
    };
    Ok((vec![vowel_move(spec)?], only))
}

const FRONT: [&str; 5] = ["IY", "IH", "EY", "EH", "AE"];
const BACK: [&str; 5] = ["UW", "UH", "OW", "AO", "AA"];
const DIPHTHONGS: [&str; 5] = ["AY", "AW", "OY", "EY", "OW"];

/// Parses a vowel move: `pull:X,Y,AMOUNT` (toward a chart point; x front 0
/// → back 1, y low 0 → high 1), `shift:DX,DY`, `rotate:DEGREES`
/// (counterclockwise), `scale:FACTOR`, `mirror:front-back`,
/// `mirror:height`.
pub fn vowel_move(spec: &str) -> Result<VowelMove, String> {
    let (kind, args) = spec.split_once(':').unwrap_or((spec, ""));
    let numbers = || -> Result<Vec<f32>, String> {
        args.split(',')
            .map(|a| a.trim().parse::<f32>().map_err(|e| format!("{spec}: {e}")))
            .collect()
    };
    let arity = |n: usize, xs: Vec<f32>| -> Result<Vec<f32>, String> {
        if xs.len() == n {
            Ok(xs)
        } else {
            Err(format!("{spec}: expected {n} numbers"))
        }
    };
    Ok(match kind {
        "pull" => {
            let v = arity(3, numbers()?)?;
            VowelMove::Pull {
                toward: (v[0], v[1]),
                amount: v[2],
            }
        }
        "shift" => {
            let v = arity(2, numbers()?)?;
            VowelMove::Shift { dx: v[0], dy: v[1] }
        }
        "onset" => {
            let v = arity(2, numbers()?)?;
            VowelMove::ShiftOnset { dx: v[0], dy: v[1] }
        }
        "rotate" => VowelMove::Rotate {
            degrees: arity(1, numbers()?)?[0],
        },
        "scale" => VowelMove::Scale {
            factor: arity(1, numbers()?)?[0],
        },
        "mirror" => match args {
            "front-back" => VowelMove::MirrorFrontBack,
            "height" => VowelMove::MirrorHeight,
            other => return Err(format!("mirror:{other}: expected front-back or height")),
        },
        other => return Err(format!("unknown vowel knob {other:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::Ph;

    #[test]
    fn every_consonant_knob_exists_and_applies() {
        let words: Vec<Vec<Ph>> = [
            "TH IH1 NG K",
            "B ER1 D",
            "M IH1 L K",
            "HH AW1 S",
            "F IH1 L M",
        ]
        .iter()
        .map(|w| Ph::parse_all(w).unwrap())
        .collect();
        for name in CONSONANT_KNOBS {
            let knob = consonant(name).unwrap_or_else(|| panic!("{name}"));
            let _ = knob.apply(&words);
        }
        let th = consonant("th-stopping").unwrap().apply(&words);
        assert_eq!(crate::phone::spell(&th[0]), "T IH1 NG K");
        let film = consonant("epenthesis").unwrap().apply(&words);
        assert_eq!(crate::phone::spell(&film[4]), "F IH1 L AH0 M");
        assert!(consonant("nope").is_none());
    }

    #[test]
    fn vowel_knobs_parse() {
        assert_eq!(
            vowel_move("pull:1,1,0.5"),
            Ok(VowelMove::Pull {
                toward: (1.0, 1.0),
                amount: 0.5
            })
        );
        assert_eq!(vowel_move("mirror:height"), Ok(VowelMove::MirrorHeight));
        assert!(vowel_move("rotate:a").is_err() && vowel_move("pull:1,2").is_err());
        let (moves, only) = vowel_knob("shift:0,0.3@tense").unwrap();
        assert_eq!(moves, [VowelMove::Shift { dx: 0.0, dy: 0.3 }]);
        assert!(
            only.unwrap()
                .matches(crate::phone::Ph::parse("IY1").unwrap())
        );
        assert!(vowel_knob("tense-up:0.2").is_ok() && vowel_knob("shift:0,1@nope").is_err());
    }
}
