//! Accent presets: bundles of rules and vowel moves that make General
//! American phonemes sound, to a General American ear, like an accent.
//!
//! Each rule cites its id in `docs/accent-research.md` (IRL-01, NZ-02, …),
//! which gives the sources, the varieties each feature belongs to, and how
//! lossy its ARPAbet rendering is. Features ARPAbet can't express (slit-t,
//! monophthongal FACE, fronted GOOSE, intonation) are left out. Rates below
//! 1 stand for variable features. These are caricatures by construction: a
//! GA listener's percept of an accent, through 39 phonemes.

use crate::deform::{Action, Class, Context, Deformation, Rule, Spelling};
use crate::knobs::consonant;

/// `(name, description)` for every preset.
pub const PRESETS: [(&str, &str); 17] = [
    (
        "irish",
        "stage Irish: tirty-tree, tay, Oirish, fillum, ish, -in'",
    ),
    (
        "irish-rural",
        "rural/West Irish: TH-stopping, boil = bile, luck ≈ look, pin = pen, MOUTH → OW",
    ),
    (
        "scottish",
        "Scottish English with a Scots layer: pull = pool, three NURSE vowels, girl → girul, hoose",
    ),
    (
        "cockney",
        "H-dropping, TH-fronting, glottal T, L-vocalization, non-rhotic, mate ≈ mite, no ≈ now",
    ),
    (
        "rp",
        "Received Pronunciation: non-rhotic, BATH → AA, LOT rounded",
    ),
    (
        "posh",
        "conservative RP: rp plus the tapped R ('veddy') and the lax happY",
    ),
    (
        "southern",
        "US South: ride ≈ rod, pin = pen, the drawl, feel = fill, -in'",
    ),
    (
        "new-zealand",
        "NZ: fush and chups, pen ≈ pin, bad ≈ bed, non-rhotic, mate ≈ mite",
    ),
    (
        "australian",
        "Australian: today ≈ to die, nice ≈ noice, non-rhotic, BATH → AA",
    ),
    (
        "northern-cities",
        "the Northern Cities Shift: stock ≈ stack, bus ≈ boss, caught ≈ cot",
    ),
    (
        "jamaican",
        "Jamaican: TH-stopping, H-dropping, cyar, cluster reduction, town ≈ tone",
    ),
    (
        "german",
        "German-accented: final devoicing, W → V, TH → S, DH → Z",
    ),
    (
        "french",
        "French-accented: H-dropping, ship = sheep, TH → S/Z, final stress",
    ),
    (
        "spanish",
        "Spanish-accented: espeak, five vowels, b = v, z = s",
    ),
    (
        "japanese",
        "Japanese-accented: l/r swap, epenthetic vowels (desuku), b = v",
    ),
    (
        "great-vowel-shift",
        "a hypothetical second Great Vowel Shift (EY → IY, IY → AY, OW → UW, UW → AW)",
    ),
    (
        "reverse-vowel-shift",
        "the Great Vowel Shift undone: IY → EY, AY → IY, AW → UW (Scots hoose, Irish tay)",
    ),
];

fn r(name: &'static str, target: Class, action: Action) -> Rule {
    Rule::new(name, target, action)
}

fn is(symbols: &[&'static str]) -> Class {
    Class::is(symbols)
}

fn stressed(symbols: &[&'static str]) -> Class {
    Class::StressedIs(symbols.to_vec())
}

fn before(symbols: &[&'static str]) -> Context {
    Context::Class(is(symbols))
}

fn vowel() -> Context {
    Context::Class(Class::Vowel)
}

/// Not followed by a vowel: the coda, roughly (CMUdict has no syllables).
fn coda() -> Context {
    Context::Not(Class::Vowel)
}

/// IRL-09 / SCO-07: a schwa between a liquid and a following sonorant.
fn epenthesis(after: &'static str, before_these: &[&'static str]) -> Rule {
    r(
        "epenthetic schwa",
        is(&[after]),
        Action::Seq(vec!["*", "AH0"]),
    )
    .right(before(before_these))
}

/// T-glottalling heard as nothing: T → ∅ between a vowel and an unstressed
/// vowel, and word-finally after a vowel.
fn t_glottal(d: Deformation, rate: f32) -> Deformation {
    d.rule(
        r("glottal T", is(&["T"]), Action::Delete)
            .left(vowel())
            .right(Context::Class(Class::UnstressedVowel)),
    )
    .at(rate)
    .rule(
        r("glottal T, final", is(&["T"]), Action::Delete)
            .left(vowel())
            .right(Context::Edge),
    )
    .at(rate)
}

/// RP-03: BATH words take PALM. The context rule (before F, TH, S)
/// over-applies to rarer words, so known TRAP words are excluded; BATH
/// words before nasals are listed.
fn bath(d: Deformation) -> Deformation {
    d.rule(
        r(
            "BATH before a fricative",
            stressed(&["AE"]),
            Action::To("AA"),
        )
        .right(before(&["F", "TH", "S"]))
        .spelled(Spelling::NotWord(vec![
            "gas",
            "mass",
            "maths",
            "math",
            "massive",
            "classic",
            "classical",
            "passage",
            "fantastic",
            "plastic",
            "elastic",
            "fascinate",
            "lass",
            "crass",
            "ass",
        ])),
    )
    .rule(
        r("BATH before a nasal", stressed(&["AE"]), Action::To("AA")).spelled(Spelling::Word(
            vec![
                "dance", "chance", "france", "plant", "grant", "aunt", "can't", "example",
                "demand", "command", "answer", "advance", "branch", "glance", "trance", "slant",
            ],
        )),
    )
}

pub fn preset(name: &str) -> Option<Deformation> {
    let d = Deformation::new(name);
    Some(match name {
        "irish" => d
            .then(consonant("th-stopping")?) // IRL-01, IRL-02
            .rule(
                r("tay: ‹ea› FLEECE → FACE", is(&["IY"]), Action::To("EY"))
                    .spelled(Spelling::Contains(vec!["ea"])),
            ) // IRL-03
            .rule(r("me for my", is(&["AY"]), Action::To("IY")).spelled(Spelling::Word(vec!["my"]))) // IRL-33
            .rule(r("Oirish: PRICE → CHOICE", is(&["AY"]), Action::To("OY"))) // IRL-04
            .rule(
                r("ould", is(&["OW"]), Action::To("AW"))
                    .right(before(&["L"]))
                    .spelled(Spelling::Contains(vec!["old"])),
            ) // IRL-06
            .rule(r("STRUT → FOOT", stressed(&["AH"]), Action::To("UH"))) // IRL-07
            .at(0.6)
            .rule(epenthesis("L", &["M", "N"])) // IRL-09
            .rule(epenthesis("R", &["M"]))
            .rule(r("ish", is(&["Z"]), Action::To("SH")).spelled(Spelling::Word(vec!["is"]))) // IRL-10
            .rule(
                r("S → SH before a consonant", is(&["S"]), Action::To("SH"))
                    .right(Context::Class(Class::Consonant)),
            ) // IRL-10
            .at(0.3)
            .then(consonant("g-dropping")?), // IRL-18
        "irish-rural" => d
            .then(consonant("th-stopping")?) // IRL-01, IRL-02
            .simultaneous(vec![
                r("CHOICE unrounded", is(&["OY"]), Action::To("AY")), // IRL-05
                r("MOUTH → OW", is(&["AW"]), Action::To("OW")),       // IRL-12
                r("THOUGHT unrounded", is(&["AO"]), Action::To("AA")), // IRL-13
                r("START fronted", is(&["AA"]), Action::To("AE")).right(before(&["R"])), // IRL-14
                r("pin–pen", is(&["EH"]), Action::To("IH")).right(before(&["M", "N"])), // IRL-11
            ])
            .rule(r("STRUT → FOOT", stressed(&["AH"]), Action::To("UH"))) // IRL-07
            .at(0.5)
            .rule(r("tay", is(&["IY"]), Action::To("EY")).spelled(Spelling::Contains(vec!["ea"]))) // IRL-03
            .at(0.5)
            .rule(epenthesis("L", &["M", "N"])) // IRL-09
            .rule(epenthesis("R", &["M"])),
        "scottish" => d
            .simultaneous(vec![
                r("FOOT = GOOSE", is(&["UH"]), Action::To("UW")), // SCO-01
                r("LOT rounded", is(&["AA"]), Action::To("AO")).right(Context::Not(is(&["R"]))), // SCO-02
                r("Scots hoose", is(&["AW"]), Action::To("UW")), // SCO-17
                r("Scots heid", is(&["EH"]), Action::To("IY"))
                    .spelled(Spelling::Contains(vec!["ea"])), // SCO-17
                r("Scots hame", is(&["OW"]), Action::To("EY")).spelled(Spelling::Word(vec![
                    "home", "stone", "bone", "alone", "no", "nobody", "go",
                ])), // SCO-17
            ])
            // SCO-06: three NURSE vowels, by spelling.
            .rule(
                r(
                    "NURSE ‹ir›",
                    stressed(&["ER"]),
                    Action::Seq(vec!["IH", "R"]),
                )
                .spelled(Spelling::Contains(vec!["ir", "yr"])),
            )
            .rule(
                r(
                    "NURSE ‹ur›",
                    stressed(&["ER"]),
                    Action::Seq(vec!["AH", "R"]),
                )
                .spelled(Spelling::Contains(vec!["ur", "or"])),
            )
            .rule(
                r(
                    "NURSE ‹er›",
                    stressed(&["ER"]),
                    Action::Seq(vec!["EH", "R"]),
                )
                .spelled(Spelling::Contains(vec!["er", "ear"])),
            )
            .rule(epenthesis("R", &["L", "M", "N"])) // SCO-07
            .rule(epenthesis("L", &["M"]))
            .rule(
                r("rolled R (heard as a tap)", is(&["R"]), Action::To("D"))
                    .left(vowel())
                    .right(Context::Class(Class::UnstressedVowel)),
            ) // SCO-05
            .at(0.4)
            .rule(
                r(
                    "happY = FACE",
                    Class::UnstressedIs(vec!["IY"]),
                    Action::To("EY"),
                )
                .right(Context::Edge),
            ) // SCO-10
            .rule(
                r("Scots aw", is(&["L"]), Action::Delete)
                    .left(before(&["AO"]))
                    .right(Context::Edge),
            ) // SCO-17
            .then(t_glottal(Deformation::new(""), 0.5)), // SCO-12
        "cockney" => d
            .then(consonant("h-dropping")?) // CKN-01
            .then(consonant("th-fronting")?) // CKN-02..04
            .then(t_glottal(Deformation::new(""), 0.8)) // CKN-05
            .then(consonant("l-vocalization")?) // CKN-06
            .then(consonant("non-rhotic")?) // CKN-07
            // CKN-09..12: the diphthong shift, at once.
            .simultaneous(vec![
                r("FACE → PRICE", is(&["EY"]), Action::To("AY")),
                r("PRICE → CHOICE", is(&["AY"]), Action::To("OY")),
                r("GOAT → MOUTH", is(&["OW"]), Action::To("AW")),
                r("MOUTH → TRAP", is(&["AW"]), Action::To("AE")),
            ])
            .rule(r("FLEECE → FACE", stressed(&["IY"]), Action::To("EY"))) // CKN-13
            .at(0.3)
            .then(consonant("g-dropping")?), // CKN-16
        "rp" => bath(d.then(consonant("non-rhotic")?)) // RP-01, RP-03
            .rule(
                r("LOT rounded", is(&["AA"]), Action::To("AO")) // RP-04
                    .right(Context::Not(is(&["R"])))
                    .spelled(Spelling::NotWord(vec![
                        "father", "palm", "calm", "spa", "bra", "balm", "psalm", "drama", "llama",
                        "pasta", "mama", "papa", "almond", "rather",
                    ])),
            ),
        "posh" => preset("rp")?
            .rule(
                r("tapped R ('veddy')", is(&["R"]), Action::To("D"))
                    .left(vowel())
                    .right(Context::Class(Class::UnstressedVowel)),
            ) // RP-10
            .rule(
                r(
                    "lax happY",
                    Class::UnstressedIs(vec!["IY"]),
                    Action::To("IH"),
                )
                .right(Context::Edge),
            ), // RP-12
        "southern" => d
            .rule(
                r("PRICE monophthong", is(&["AY"]), Action::To("AA"))
                    .right(Context::Not(Class::VoicelessObstruent)),
            ) // SUS-01
            .simultaneous(vec![
                r("pin–pen", is(&["EH"]), Action::To("IH")).right(before(&["M", "N", "NG"])), // SUS-02
                r("feel = fill", is(&["IY"]), Action::To("IH")).right(before(&["L"])), // SUS-07
                r("fail = fell", is(&["EY"]), Action::To("EH")).right(before(&["L"])), // SUS-07
                r("boil = ball", is(&["OY"]), Action::To("AO")).right(before(&["L"])), // SUS-08
            ])
            // SUS-03: the drawl breaks stressed short front vowels.
            .simultaneous(vec![
                r(
                    "drawl KIT",
                    stressed(&["IH"]),
                    Action::Seq(vec!["IY", "AH0"]),
                ),
                r(
                    "drawl DRESS",
                    stressed(&["EH"]),
                    Action::Seq(vec!["EY", "IH0"]),
                ),
                r(
                    "drawl TRAP",
                    stressed(&["AE"]),
                    Action::Seq(vec!["*", "Y", "AH0"]),
                ),
            ])
            .at(0.4)
            .then(consonant("g-dropping")?), // SUS-09
        "new-zealand" => d
            // NZ-01..03, from the top of the chain down so nothing cascades.
            .rule(
                r("KIT centralized", stressed(&["IH"]), Action::To("AH"))
                    .right(Context::Not(is(&["NG"]))),
            )
            .rule(r("DRESS raised", is(&["EH"]), Action::To("IH")))
            .rule(r("TRAP raised", is(&["AE"]), Action::To("EH")))
            .then(consonant("non-rhotic")?) // NZ-05
            .rule(r("PRICE → CHOICE", is(&["AY"]), Action::To("OY"))) // NZ-07
            .at(0.5)
            .rule(r("FACE → PRICE", is(&["EY"]), Action::To("AY"))) // NZ-06
            .at(0.5)
            .rule(r("GOAT → MOUTH", is(&["OW"]), Action::To("AW"))) // NZ-08
            .at(0.6),
        "australian" => bath(d.then(consonant("non-rhotic")?)) // AUS-06, AUS-07
            .simultaneous(vec![
                r("FACE → PRICE", is(&["EY"]), Action::To("AY")), // AUS-01
                r("PRICE → CHOICE", is(&["AY"]), Action::To("OY")), // AUS-02
                r("GOAT → MOUTH", is(&["OW"]), Action::To("AW")), // AUS-03
            ])
            .rule(
                r(
                    "weak-vowel merger",
                    Class::UnstressedIs(vec!["IH"]),
                    Action::To("AH"),
                )
                .right(Context::Not(is(&["K", "G", "NG"]))),
            ) // AUS-12
            .then(consonant("l-vocalization")?.intensity(0.4)), // AUS-09
        "northern-cities" => d.simultaneous(vec![
            r("TRAP raised", is(&["AE"]), Action::To("EH")), // NCS-01
            r("LOT fronted", is(&["AA"]), Action::To("AE")).right(Context::Not(is(&["R"]))), // NCS-02
            r("THOUGHT lowered", is(&["AO"]), Action::To("AA")), // NCS-03
            r("DRESS backed", is(&["EH"]), Action::To("AH")),    // NCS-04
            r("STRUT backed", stressed(&["AH"]), Action::To("AO")), // NCS-05
            r("KIT lowered", stressed(&["IH"]), Action::To("EH")), // NCS-06
        ]),
        "jamaican" => d
            .then(consonant("th-stopping")?) // JAM-01
            .then(consonant("h-dropping")?.intensity(0.6)) // JAM-02
            // JAM-03: final clusters lose their last stop.
            .rule(
                r("final cluster", is(&["T"]), Action::Delete)
                    .left(before(&["S", "K", "F", "P"]))
                    .right(Context::Edge),
            )
            .rule(
                r("final cluster", is(&["D"]), Action::Delete)
                    .left(before(&["N", "L"]))
                    .right(Context::Edge),
            )
            .rule(
                r("final cluster", is(&["K"]), Action::Delete)
                    .left(before(&["S"]))
                    .right(Context::Edge),
            )
            // JAM-04: semi-rhotic.
            .rule(
                r("R before a consonant", is(&["R"]), Action::Delete)
                    .left(vowel())
                    .right(Context::Class(Class::Consonant)),
            )
            .rule(r(
                "lettER",
                Class::UnstressedIs(vec!["ER"]),
                Action::To("AH"),
            ))
            .rule(
                r("cyar", is(&["K", "G"]), Action::Seq(vec!["*", "Y"]))
                    .right(before(&["AA", "AE"])),
            ) // JAM-05
            .simultaneous(vec![
                r("TRAP → PALM", is(&["AE"]), Action::To("AA")), // JAM-06
                r("MOUTH → GOAT", is(&["AW"]), Action::To("OW")), // JAM-09
                r("STRUT rounded", stressed(&["AH"]), Action::To("UH")), // JAM-08
            ])
            .rule(r("PRICE → CHOICE", is(&["AY"]), Action::To("OY"))) // JAM-12
            .at(0.5)
            .rule(r(
                "full schwa",
                Class::UnstressedIs(vec!["AH"]),
                Action::To("AA"),
            )) // JAM-14
            .at(0.6)
            .then(consonant("g-dropping")?), // JAM-18
        "german" => d
            .rule(r("TH → S", is(&["TH"]), Action::To("S"))) // DE-04
            .rule(r("DH → Z", is(&["DH"]), Action::To("Z")))
            .then(consonant("w-to-v")?) // DE-02
            .rule(r("TRAP = DRESS", is(&["AE"]), Action::To("EH"))) // DE-05
            .rule(r("JH → CH", is(&["JH"]), Action::To("CH"))) // DE-06
            .rule(r("ZH → SH", is(&["ZH"]), Action::To("SH")))
            .rule(
                r("coda R vocalized", is(&["R"]), Action::Delete)
                    .left(vowel())
                    .right(coda()),
            ) // DE-07
            .rule(r(
                "lettER",
                Class::UnstressedIs(vec!["ER"]),
                Action::To("AH"),
            ))
            .then(consonant("final-devoicing")?), // DE-01
        "french" => d
            .rule(r("H-deletion", is(&["HH"]), Action::Delete)) // FR-01
            .rule(r("TH → S", is(&["TH"]), Action::To("S"))) // FR-02
            .rule(r("DH → Z", is(&["DH"]), Action::To("Z")))
            .simultaneous(vec![
                r("ship = sheep", is(&["IH"]), Action::To("IY")), // FR-03
                r("full = fool", is(&["UH"]), Action::To("UW")),  // FR-04
                r("CH → SH", is(&["CH"]), Action::To("SH")),      // FR-07
                r("JH → ZH", is(&["JH"]), Action::To("ZH")),
            ])
            .step(crate::deform::StepKind::FinalStress), // FR-05
        "spanish" => d
            .rule(
                r("espeak", is(&["S"]), Action::Seq(vec!["EH0", "S"]))
                    .left(Context::Edge)
                    .right(Context::Class(Class::Consonant)),
            ) // ES-01
            .simultaneous(vec![
                // ES-02: five vowels.
                r("KIT → FLEECE", is(&["IH"]), Action::To("IY")),
                r("FOOT → GOOSE", is(&["UH"]), Action::To("UW")),
                r("TRAP → PALM", is(&["AE"]), Action::To("AA")),
                r("STRUT → PALM", stressed(&["AH"]), Action::To("AA")),
                r("THOUGHT → GOAT", is(&["AO"]), Action::To("OW")),
                r("b = v", is(&["V"]), Action::To("B")), // ES-03
                r("s = z", is(&["Z"]), Action::To("S")), // ES-04
                r("TH → T", is(&["TH"]), Action::To("T")), // ES-06
                r("SH → CH", is(&["SH"]), Action::To("CH")), // ES-08
            ])
            .rule(r("DH → D", is(&["DH"]), Action::To("D")).left(Context::Not(Class::Vowel))), // ES-05
        "japanese" => d
            .simultaneous(vec![
                r("l/r", is(&["L"]), Action::To("R")), // JA-01
                r("r/l", is(&["R"]), Action::To("L")),
                r("TH → SH before IY", is(&["TH"]), Action::To("SH")).right(before(&["IY"])), // JA-03
                r("TH → S", is(&["TH"]), Action::To("S")),
                r("DH → Z", is(&["DH"]), Action::To("Z")),
                r("b = v", is(&["V"]), Action::To("B")), // JA-04
                r("si → shi", is(&["S"]), Action::To("SH")).right(before(&["IY", "IH"])), // JA-05
                r("zi → ji", is(&["Z"]), Action::To("JH")).right(before(&["IY", "IH"])),
                r("ti → chi", is(&["T"]), Action::To("CH")).right(before(&["IY", "IH"])), // JA-06
                r("hu → fu", is(&["HH"]), Action::To("F")).right(before(&["UW"])),        // JA-07
            ])
            .simultaneous(vec![
                // JA-08: vowels.
                r("TRAP → PALM", is(&["AE"]), Action::To("AA")),
                r("STRUT → PALM", stressed(&["AH"]), Action::To("AA")),
                r("THOUGHT → GOAT", is(&["AO"]), Action::To("OW")),
                r("KIT → FLEECE", is(&["IH"]), Action::To("IY")),
                r("FOOT → GOOSE", is(&["UH"]), Action::To("UW")),
                r("NURSE → PALM", is(&["ER"]), Action::To("AA")),
            ])
            // JA-02, JA-09: a vowel after every consonant that ends a
            // syllable, except N.
            .simultaneous(vec![
                r("final NG", is(&["NG"]), Action::Seq(vec!["N", "G", "UW0"])).right(Context::Edge),
                r(
                    "+o after T/D",
                    is(&["T", "D"]),
                    Action::Seq(vec!["*", "OW0"]),
                )
                .right(coda()),
                r(
                    "+i after CH/JH",
                    is(&["CH", "JH"]),
                    Action::Seq(vec!["*", "IY0"]),
                )
                .right(coda()),
                r(
                    "+u after other consonants (not N)",
                    is(&[
                        "B", "DH", "F", "G", "HH", "K", "L", "M", "P", "R", "S", "SH", "TH", "V",
                        "W", "Y", "Z", "ZH", "NG",
                    ]),
                    Action::Seq(vec!["*", "UW0"]),
                )
                .right(coda()),
            ]),
        "great-vowel-shift" => d.simultaneous(vec![
            r("FACE → FLEECE", is(&["EY"]), Action::To("IY")),
            r("FLEECE → PRICE", is(&["IY"]), Action::To("AY")),
            r("GOAT → GOOSE", is(&["OW"]), Action::To("UW")),
            r("THOUGHT → GOOSE", is(&["AO"]), Action::To("UW")),
            r("GOOSE → MOUTH", is(&["UW"]), Action::To("AW")),
        ]),
        "reverse-vowel-shift" => d.simultaneous(vec![
            r("FLEECE → FACE", is(&["IY"]), Action::To("EY")),
            r("FACE → PALM", is(&["EY"]), Action::To("AA")),
            r("PRICE → FLEECE", is(&["AY"]), Action::To("IY")),
            r("GOOSE → GOAT", is(&["UW"]), Action::To("OW")),
            r("GOAT → THOUGHT", is(&["OW"]), Action::To("AO")),
            r("MOUTH → GOOSE", is(&["AW"]), Action::To("UW")),
        ]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::{Ph, spell};

    fn deform(accent: &str, words: &[(&str, &str)]) -> Vec<String> {
        let phonemes: Vec<Vec<Ph>> = words
            .iter()
            .map(|(_, p)| Ph::parse_all(p).unwrap())
            .collect();
        let spellings: Vec<String> = words.iter().map(|(w, _)| w.to_string()).collect();
        preset(accent)
            .unwrap()
            .apply_spelled(&phonemes, &spellings)
            .iter()
            .map(|w| spell(w))
            .collect()
    }

    #[test]
    fn every_preset_builds() {
        for (name, _) in PRESETS {
            assert!(preset(name).is_some(), "{name}");
        }
    }

    #[test]
    fn signature_features() {
        // Irish: thirty-three → tirty-tree; tea → tay; film → fillum.
        let irish = deform(
            "irish",
            &[
                ("thirty", "TH ER1 T IY0"),
                ("tea", "T IY1"),
                ("film", "F IH1 L M"),
            ],
        );
        assert_eq!(irish, ["T ER1 T IY0", "T EY1", "F IH1 L AH0 M"]);
        // NZ: the chain doesn't cascade: pen → pin, bad → bed, fish → fush.
        let nz = deform(
            "new-zealand",
            &[("pen", "P EH1 N"), ("bad", "B AE1 D"), ("fish", "F IH1 SH")],
        );
        assert_eq!(nz, ["P IH1 N", "B EH1 D", "F AH1 SH"]);
        // Northern Cities: stock → stack, bus → boss, and TRAP → DRESS
        // without continuing to STRUT.
        let ncs = deform(
            "northern-cities",
            &[
                ("stock", "S T AA1 K"),
                ("bus", "B AH1 S"),
                ("cat", "K AE1 T"),
            ],
        );
        assert_eq!(ncs, ["S T AE1 K", "B AO1 S", "K EH1 T"]);
        // Cockney: think → fink, house → ouse, lake → like.
        let cockney = deform(
            "cockney",
            &[
                ("think", "TH IH1 NG K"),
                ("house", "HH AW1 S"),
                ("lake", "L EY1 K"),
            ],
        );
        assert_eq!(cockney, ["F IH1 NG K", "AE1 S", "L AY1 K"]);
        // Japanese: desk → desuku.
        let ja = deform("japanese", &[("desk", "D EH1 S K")]);
        assert_eq!(ja, ["D EH1 S UW0 K UW0"]);
    }
}
