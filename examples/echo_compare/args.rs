//! Command-line options and the echo-config override.

use std::path::PathBuf;

use mime_reader::echo::{AlignerKind, EchoConfig, EchoNormalization};
use serde::Deserialize;

pub const USAGE: &str = "\
echo_compare — score the labeled echo corpus with each aligner

USAGE: cargo run --release --example echo_compare -- [OPTIONS]

  --corpus DIR       poem directory (repeatable; default data/echo/public,
                     plus data/echo/private when present)
  --fixture FILE     synthetic pair fixture (default echo_pair_tests.json);
                     --no-fixture to skip
  --config FILE      JSON echo-config override (also: ECHO_CONFIG='{...}')
  --aligner NAME     semi_global | waterman_eggert | jump | all (default all)
  --no-mutations     skip synthetic mutations of corpus lines
  --seed N           mutation seed (default 1)
  --max-lines N      skip poems longer than N lines
  --by-poem          print the per-poem ranking table
  --dump FILE        write every scored pair (.csv or .json)
  -h, --help         this text";

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub corpora: Vec<PathBuf>,
    pub fixture: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub aligners: Vec<AlignerKind>,
    pub mutations: bool,
    pub seed: u64,
    pub max_lines: Option<usize>,
    pub by_poem: bool,
    pub dump: Option<PathBuf>,
    pub help: bool,
}

/// A path relative to the crate root.
pub fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

impl Default for Args {
    fn default() -> Self {
        let private = repo_path("data/echo/private");
        let mut corpora = vec![repo_path("data/echo/public")];
        if private.is_dir() {
            corpora.push(private);
        }
        Self {
            corpora,
            fixture: Some(repo_path("echo_pair_tests.json")),
            config: None,
            aligners: AlignerKind::ALL.to_vec(),
            mutations: true,
            seed: 1,
            max_lines: None,
            by_poem: false,
            dump: None,
            help: false,
        }
    }
}

impl Args {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = Self::default();
        let mut explicit_corpora = Vec::new();
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            let mut value = |name: &str| {
                arguments
                    .next()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match flag.as_str() {
                "--corpus" => explicit_corpora.push(PathBuf::from(value("--corpus")?)),
                "--fixture" => args.fixture = Some(PathBuf::from(value("--fixture")?)),
                "--no-fixture" => args.fixture = None,
                "--config" => args.config = Some(PathBuf::from(value("--config")?)),
                "--aligner" => {
                    let name = value("--aligner")?;
                    args.aligners = if name == "all" {
                        AlignerKind::ALL.to_vec()
                    } else {
                        vec![name.parse()?]
                    };
                }
                "--no-mutations" => args.mutations = false,
                "--seed" => {
                    args.seed = value("--seed")?
                        .parse()
                        .map_err(|e| format!("--seed: {e}"))?
                }
                "--max-lines" => {
                    args.max_lines = Some(
                        value("--max-lines")?
                            .parse()
                            .map_err(|e| format!("--max-lines: {e}"))?,
                    )
                }
                "--by-poem" => args.by_poem = true,
                "--dump" => args.dump = Some(PathBuf::from(value("--dump")?)),
                "-h" | "--help" => args.help = true,
                other => return Err(format!("unknown option {other:?}\n\n{USAGE}")),
            }
        }
        if !explicit_corpora.is_empty() {
            args.corpora = explicit_corpora;
        }
        Ok(args)
    }
}

/// Echo-config fields to override, flat and all optional. Costs are in
/// thousandths of a unit, as in [`EchoConfig`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigOverride {
    pub vowel_coefficient: Option<u32>,
    pub consonant_coefficient: Option<u32>,
    pub vowel_zero_point: Option<u32>,
    pub consonant_zero_point: Option<u32>,
    pub primary_weight: Option<u32>,
    pub secondary_weight: Option<u32>,
    pub unstressed_weight: Option<u32>,
    pub gap_open: Option<u32>,
    pub gap_extend: Option<u32>,
    pub gap_open_repeated_consonant: Option<u32>,
    pub move_cost: Option<u32>,
    pub run_open: Option<u32>,
    pub min_run: Option<u32>,
    pub reuse_penalty: Option<u32>,
    pub max_runs: Option<usize>,
    /// "shorter" or "mean".
    pub normalization: Option<String>,
    pub max_variants_per_line: Option<usize>,
    pub max_variant_pairs: Option<usize>,
}

impl ConfigOverride {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("echo config override: {e}"))
    }

    pub fn apply(&self, config: &mut EchoConfig) -> Result<(), String> {
        fn set<T: Copy>(target: &mut T, value: Option<T>) {
            if let Some(value) = value {
                *target = value;
            }
        }
        let s = &mut config.similarity;
        set(&mut s.vowel_coefficient, self.vowel_coefficient);
        set(&mut s.consonant_coefficient, self.consonant_coefficient);
        set(&mut s.vowel_zero_point, self.vowel_zero_point);
        set(&mut s.consonant_zero_point, self.consonant_zero_point);
        set(&mut s.stress_weights.primary, self.primary_weight);
        set(&mut s.stress_weights.secondary, self.secondary_weight);
        set(&mut s.stress_weights.unstressed, self.unstressed_weight);
        let g = &mut config.gaps;
        set(&mut g.gap_open, self.gap_open);
        set(&mut g.gap_extend, self.gap_extend);
        set(
            &mut g.gap_open_repeated_consonant,
            self.gap_open_repeated_consonant,
        );
        let t = &mut config.structure;
        set(&mut t.move_cost, self.move_cost);
        set(&mut t.run_open, self.run_open);
        set(&mut t.min_run, self.min_run);
        set(&mut t.reuse_penalty, self.reuse_penalty);
        set(&mut t.max_runs, self.max_runs);
        set(
            &mut config.max_variants_per_line,
            self.max_variants_per_line,
        );
        set(&mut config.max_variant_pairs, self.max_variant_pairs);
        if let Some(normalization) = &self.normalization {
            config.normalization = match normalization.as_str() {
                "shorter" => EchoNormalization::ShorterSelfSimilarity,
                "mean" => EchoNormalization::MeanSelfSimilarity,
                other => return Err(format!("normalization {other:?}: expected shorter or mean")),
            };
        }
        Ok(())
    }
}

/// The effective config: defaults, then `--config FILE`, then `ECHO_CONFIG`.
pub fn effective_config(args: &Args, env: Option<String>) -> Result<EchoConfig, String> {
    let mut config = EchoConfig::default();
    if let Some(path) = &args.config {
        let json = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        ConfigOverride::from_json(&json)?.apply(&mut config)?;
    }
    if let Some(json) = env {
        ConfigOverride::from_json(&json)?.apply(&mut config)?;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_cover_everything() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.aligners, AlignerKind::ALL);
        assert!(args.mutations);
        assert!(args.corpora[0].ends_with("data/echo/public"));
        assert!(args.fixture.is_some());
    }

    #[test]
    fn options_parse() {
        let args = parse(&[
            "--aligner",
            "jump",
            "--no-mutations",
            "--seed",
            "7",
            "--max-lines",
            "40",
            "--corpus",
            "a",
            "--corpus",
            "b",
            "--by-poem",
            "--dump",
            "x.csv",
            "--no-fixture",
        ])
        .unwrap();
        assert_eq!(args.aligners, [AlignerKind::Jump]);
        assert!(!args.mutations);
        assert_eq!((args.seed, args.max_lines), (7, Some(40)));
        assert_eq!(args.corpora, [PathBuf::from("a"), PathBuf::from("b")]);
        assert!(args.by_poem && args.fixture.is_none());
        assert_eq!(args.dump, Some(PathBuf::from("x.csv")));
        assert!(parse(&["--aligner", "nope"]).is_err());
        assert!(parse(&["--seed"]).is_err());
        assert!(parse(&["--frobnicate"]).is_err());
    }

    #[test]
    fn overrides_apply_on_top_of_defaults() {
        let args = parse(&[]).unwrap();
        let config = effective_config(
            &args,
            Some(r#"{"gap_open":250,"normalization":"mean"}"#.into()),
        )
        .unwrap();
        assert_eq!(config.gaps.gap_open, 250);
        assert_eq!(config.normalization, EchoNormalization::MeanSelfSimilarity);
        assert_eq!(config.structure, EchoConfig::default().structure);
        assert!(effective_config(&args, Some(r#"{"gap_opne":1}"#.into())).is_err());
    }
}
