//! Command-line options and the echo-config override.

use std::path::PathBuf;

use mime_reader::echo::{AlignerKind, EchoConfig, EchoNormalization};
use serde::{Deserialize, Serialize};

pub const USAGE: &str = "\
echo_compare — score the labeled echo corpus with each aligner

USAGE: cargo run --release --example echo_compare -- [OPTIONS]

  --corpus DIR       poem directory (repeatable; default data/echo/public,
                     plus data/echo/private when present)
  --fixture FILE     synthetic pair fixture (default echo_pair_tests.json);
                     --no-fixture to skip
  --config FILE      JSON echo-config override, applied on top of each
                     aligner's tuned config (also: ECHO_CONFIG='{...}')
  --aligner NAME     semi_global | waterman_eggert | all (default all)
  --no-mutations     skip synthetic mutations of corpus lines
  --seed N           mutation seed (default 1)
  --max-lines N      skip poems longer than N lines
  --forms A,B        only use poems of these forms (e.g. pantoum,duplex)
  --by-poem          print the per-poem ranking table
  --dump FILE        write every scored pair (.csv or .json)
  --threads N        worker threads (default: all cores)
  -h, --help         this text

PARAMETER SEARCH (replaces the report):
  --search N         try N random configs per aligner, then refine the best;
                     0 scores just the base config per fold
  --search-seed N    search seed (default 1)
  --folds K          cross-validation folds, stratified by form and strictness
                     (default 5)
  --objective NAME   strictness (mean of strict and loose AUC; default) or
                     form (mean of per-form AUCs)";

/// What the parameter search averages AUC over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    /// Strict poems and loose poems, equally — each form counts within its
    /// pool by its number of pairs.
    Strictness,
    /// Each form equally.
    Form,
}

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
    pub search: Option<usize>,
    pub search_seed: u64,
    pub folds: usize,
    pub objective: Objective,
    pub forms: Option<Vec<String>>,
    pub threads: usize,
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
            search: None,
            search_seed: 1,
            folds: 5,
            objective: Objective::Strictness,
            forms: None,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
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
                "--search" => {
                    args.search = Some(
                        value("--search")?
                            .parse()
                            .map_err(|e| format!("--search: {e}"))?,
                    )
                }
                "--search-seed" => {
                    args.search_seed = value("--search-seed")?
                        .parse()
                        .map_err(|e| format!("--search-seed: {e}"))?
                }
                "--folds" => {
                    args.folds = value("--folds")?
                        .parse::<usize>()
                        .map_err(|e| format!("--folds: {e}"))?;
                    if args.folds < 2 {
                        return Err("--folds must be at least 2".into());
                    }
                }
                "--objective" => {
                    args.objective = match value("--objective")?.as_str() {
                        "strictness" => Objective::Strictness,
                        "form" => Objective::Form,
                        other => {
                            return Err(format!(
                                "--objective {other:?}: expected strictness or form"
                            ));
                        }
                    }
                }
                "--forms" => {
                    args.forms = Some(
                        value("--forms")?
                            .split(',')
                            .map(|f| f.trim().to_lowercase())
                            .filter(|f| !f.is_empty())
                            .collect(),
                    )
                }
                "--threads" => {
                    args.threads = value("--threads")?
                        .parse::<usize>()
                        .map_err(|e| format!("--threads: {e}"))?
                        .max(1)
                }
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vowel_coefficient: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consonant_coefficient: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vowel_zero_point: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consonant_zero_point: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_weight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_weight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unstressed_weight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_open: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_extend: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_open_repeated_consonant: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_open: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_run: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_runs: Option<usize>,
    /// "shorter" or "mean".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_variants_per_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
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

/// The effective config for `kind`: its tuned config, then `--config FILE`,
/// then `ECHO_CONFIG`.
pub fn effective_config(
    args: &Args,
    env: Option<&str>,
    kind: AlignerKind,
) -> Result<EchoConfig, String> {
    let mut config = EchoConfig::for_aligner(kind);
    if let Some(path) = &args.config {
        let json = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        ConfigOverride::from_json(&json)?.apply(&mut config)?;
    }
    if let Some(json) = env {
        ConfigOverride::from_json(json)?.apply(&mut config)?;
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
            "waterman_eggert",
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
        assert_eq!(args.aligners, [AlignerKind::WatermanEggert]);
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
    fn overrides_apply_on_top_of_each_aligners_tuned_config() {
        let args = parse(&[]).unwrap();
        for kind in AlignerKind::ALL {
            let tuned = EchoConfig::for_aligner(kind);
            assert_eq!(effective_config(&args, None, kind).unwrap(), tuned);
            let config = effective_config(
                &args,
                Some(r#"{"gap_open":250,"normalization":"shorter"}"#),
                kind,
            )
            .unwrap();
            assert_eq!(config.gaps.gap_open, 250);
            assert_eq!(
                config.normalization,
                EchoNormalization::ShorterSelfSimilarity
            );
            assert_eq!(config.structure, tuned.structure);
            assert_eq!(config.aligner, kind);
            assert!(effective_config(&args, Some(r#"{"gap_opne":1}"#), kind).is_err());
        }
    }
}
