//! Deformations by name, as the command line and the web page give them:
//! a list of steps (accent presets, consonant knobs, vowel-space moves),
//! an intensity and a seed.

use crate::deform::Deformation;
use crate::{accents, knobs};

/// One step of a deformation.
#[derive(Debug, Clone, PartialEq)]
pub enum StepSpec {
    /// An accent preset ([`accents::PRESETS`]).
    Accent(String),
    /// A consonant knob ([`knobs::CONSONANT_KNOBS`]).
    Knob(String),
    /// A vowel move, as [`knobs::vowel_knob`] parses it.
    Vowels(String),
}

impl StepSpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Accent(s) | Self::Knob(s) | Self::Vowels(s) => s,
        }
    }
}

/// The deformation `steps` describe, with every rate scaled by `intensity`
/// and partial rates picking sites by `seed`.
pub fn deformation(steps: &[StepSpec], intensity: f32, seed: u64) -> Result<Deformation, String> {
    let mut d = Deformation::new("");
    for step in steps {
        let part = match step {
            StepSpec::Accent(name) => {
                accents::preset(name).ok_or_else(|| format!("unknown accent {name:?}"))?
            }
            StepSpec::Knob(name) => {
                knobs::consonant(name).ok_or_else(|| format!("unknown knob {name:?}"))?
            }
            StepSpec::Vowels(spec) => {
                let (moves, only) = knobs::vowel_knob(spec)?;
                Deformation::new(spec.clone()).vowels_only(moves, only)
            }
        };
        d = d.then(part);
    }
    let mut d = d.intensity(intensity);
    d.seed = seed;
    d.name = steps
        .iter()
        .map(StepSpec::name)
        .collect::<Vec<_>>()
        .join(" + ");
    if intensity < 1.0 {
        d.name += &format!(" @ {intensity}");
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_build_deformations() {
        let d = deformation(
            &[
                StepSpec::Accent("irish".into()),
                StepSpec::Knob("h-dropping".into()),
                StepSpec::Vowels("pull:1,1,0.5@lax".into()),
            ],
            0.5,
            7,
        )
        .unwrap();
        assert_eq!(d.name, "irish + h-dropping + pull:1,1,0.5@lax @ 0.5");
        assert_eq!(d.seed, 7);
        assert!(deformation(&[StepSpec::Accent("martian".into())], 1.0, 0).is_err());
        assert!(deformation(&[StepSpec::Vowels("spin:3".into())], 1.0, 0).is_err());
    }
}
