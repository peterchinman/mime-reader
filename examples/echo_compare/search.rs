//! Parameter search ("fuzzing") over echo configs.
//!
//! Poems are split, per form, into training and held-out poems. Each
//! candidate config scores every pair; its objective is the mean over forms
//! of the AUC (echo vs non-echo) on the training poems, so a large form
//! doesn't drown a small one. The held-out AUC is reported next to it, as
//! an honest estimate of what the search found rather than what it fit.
//!
//! Random sampling over the parameters that matter to the aligner comes
//! first; then rounds of local perturbation around the best few configs.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::Instant;

use mime_reader::echo::{AlignerKind, EchoConfig, EchoDetector};
use mime_reader::line::Line;

use crate::args::{Args, ConfigOverride, repo_path};
use crate::corpus::{Label, LabeledPair, Poem};
use crate::metrics;
use crate::mutate::Rng;

/// A searchable integer parameter: its name in [`ConfigOverride`] and its
/// range.
struct Param {
    name: &'static str,
    lo: u32,
    hi: u32,
}

const SIMILARITY: [Param; 4] = [
    Param {
        name: "vowel_coefficient",
        lo: 1,
        hi: 8,
    },
    Param {
        name: "consonant_coefficient",
        lo: 1,
        hi: 8,
    },
    Param {
        name: "vowel_zero_point",
        lo: 150,
        hi: 700,
    },
    Param {
        name: "consonant_zero_point",
        lo: 300,
        hi: 1000,
    },
];
const STRESS: [Param; 2] = [
    Param {
        name: "secondary_weight",
        lo: 1,
        hi: 4,
    },
    Param {
        name: "unstressed_weight",
        lo: 1,
        hi: 4,
    },
];
const GAPS: [Param; 3] = [
    Param {
        name: "gap_open",
        lo: 0,
        hi: 1500,
    },
    Param {
        name: "gap_extend",
        lo: 0,
        hi: 200,
    },
    Param {
        name: "gap_open_repeated_consonant",
        lo: 0,
        hi: 1500,
    },
];
const RUNS: [Param; 3] = [
    Param {
        name: "move_cost",
        lo: 0,
        hi: 1500,
    },
    Param {
        name: "run_open",
        lo: 0,
        hi: 1500,
    },
    Param {
        name: "min_run",
        lo: 200,
        hi: 1500,
    },
];
const JUMPS: [Param; 2] = [
    Param {
        name: "move_cost",
        lo: 0,
        hi: 1500,
    },
    Param {
        name: "reuse_penalty",
        lo: 0,
        hi: 1500,
    },
];

/// The parameters that affect `kind`.
fn params(kind: AlignerKind) -> Vec<&'static Param> {
    let mut all: Vec<&Param> = SIMILARITY.iter().chain(&STRESS).chain(&GAPS).collect();
    match kind {
        AlignerKind::SemiGlobal => {}
        AlignerKind::WatermanEggert => all.extend(&RUNS),
        AlignerKind::Jump => all.extend(&JUMPS),
    }
    all
}

fn slot<'o>(o: &'o mut ConfigOverride, name: &str) -> &'o mut Option<u32> {
    match name {
        "vowel_coefficient" => &mut o.vowel_coefficient,
        "consonant_coefficient" => &mut o.consonant_coefficient,
        "vowel_zero_point" => &mut o.vowel_zero_point,
        "consonant_zero_point" => &mut o.consonant_zero_point,
        "secondary_weight" => &mut o.secondary_weight,
        "unstressed_weight" => &mut o.unstressed_weight,
        "gap_open" => &mut o.gap_open,
        "gap_extend" => &mut o.gap_extend,
        "gap_open_repeated_consonant" => &mut o.gap_open_repeated_consonant,
        "move_cost" => &mut o.move_cost,
        "run_open" => &mut o.run_open,
        "min_run" => &mut o.min_run,
        "reuse_penalty" => &mut o.reuse_penalty,
        other => unreachable!("unknown search parameter {other}"),
    }
}

fn uniform(rng: &mut Rng) -> f64 {
    (rng.next() >> 11) as f64 / (1u64 << 53) as f64
}

/// Keeps a sampled config coherent: unstressed vowels weigh no more than
/// secondary ones, and a repeated-consonant gap opens no dearer than any
/// other.
fn constrain(o: &mut ConfigOverride) {
    if let (Some(s), Some(u)) = (o.secondary_weight, o.unstressed_weight) {
        o.unstressed_weight = Some(u.min(s));
    }
    if let (Some(open), Some(r)) = (o.gap_open, o.gap_open_repeated_consonant) {
        o.gap_open_repeated_consonant = Some(r.min(open));
    }
}

/// A config drawn uniformly from the ranges of `kind`'s parameters.
fn sample(kind: AlignerKind, rng: &mut Rng) -> ConfigOverride {
    let mut o = ConfigOverride::default();
    for p in params(kind) {
        *slot(&mut o, p.name) = Some(rng.range(p.lo as usize..p.hi as usize + 1) as u32);
    }
    // Per-phoneme gap cost: half the time none, as in the default.
    if uniform(rng) < 0.5 {
        o.gap_extend = Some(0);
    }
    o.normalization = Some(
        if uniform(rng) < 0.5 {
            "shorter"
        } else {
            "mean"
        }
        .to_string(),
    );
    constrain(&mut o);
    o
}

/// `base` with each parameter nudged, with probability ½, by up to
/// `scale` of its range.
fn perturb(kind: AlignerKind, base: &ConfigOverride, scale: f64, rng: &mut Rng) -> ConfigOverride {
    let mut o = base.clone();
    for p in params(kind) {
        if uniform(rng) < 0.5 {
            continue;
        }
        let span = f64::from(p.hi - p.lo);
        let current = f64::from(slot(&mut o, p.name).unwrap_or(p.lo));
        let step = (uniform(rng) * 2.0 - 1.0) * scale * span;
        let next = (current + step)
            .round()
            .clamp(f64::from(p.lo), f64::from(p.hi));
        *slot(&mut o, p.name) = Some(next as u32);
    }
    if uniform(rng) < 0.1 {
        o.normalization = Some(match o.normalization.as_deref() {
            Some("mean") => "shorter".to_string(),
            _ => "mean".to_string(),
        });
    }
    constrain(&mut o);
    o
}

/// Every override field `kind` searches, filled from `config` (so the
/// defaults print and perturb like any sample).
fn override_of(kind: AlignerKind, config: &EchoConfig) -> ConfigOverride {
    let mut o = ConfigOverride::default();
    let value = |name: &str| -> u32 {
        let s = &config.similarity;
        match name {
            "vowel_coefficient" => s.vowel_coefficient,
            "consonant_coefficient" => s.consonant_coefficient,
            "vowel_zero_point" => s.vowel_zero_point,
            "consonant_zero_point" => s.consonant_zero_point,
            "secondary_weight" => s.stress_weights.secondary,
            "unstressed_weight" => s.stress_weights.unstressed,
            "gap_open" => config.gaps.gap_open,
            "gap_extend" => config.gaps.gap_extend,
            "gap_open_repeated_consonant" => config.gaps.gap_open_repeated_consonant,
            "move_cost" => config.structure.move_cost,
            "run_open" => config.structure.run_open,
            "min_run" => config.structure.min_run,
            "reuse_penalty" => config.structure.reuse_penalty,
            other => unreachable!("{other}"),
        }
    };
    for p in params(kind) {
        *slot(&mut o, p.name) = Some(value(p.name));
    }
    o.normalization = Some(
        match config.normalization {
            mime_reader::echo::EchoNormalization::ShorterSelfSimilarity => "shorter",
            mime_reader::echo::EchoNormalization::MeanSelfSimilarity => "mean",
        }
        .to_string(),
    );
    o
}

/// A pair to score: indices into the shared line table, its label, and the
/// poem it came from (`None` for fixture pairs).
#[derive(Clone, Copy)]
pub struct Job {
    pub a: usize,
    pub b: usize,
    pub label: Label,
    pub poem: Option<usize>,
}

/// Scores every job with `kind` under `detector`, spread over `threads`.
fn score_all(
    detector: &EchoDetector,
    kind: AlignerKind,
    jobs: &[Job],
    lines: &[Line],
    threads: usize,
) -> Vec<f32> {
    let chunk = jobs.len().div_ceil(threads.max(1)).max(1);
    let mut scores = vec![0.0f32; jobs.len()];
    std::thread::scope(|scope| {
        for (jobs, out) in jobs.chunks(chunk).zip(scores.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (job, slot) in jobs.iter().zip(out) {
                    *slot = detector
                        .compare_lines_with(kind, &lines[job.a], &lines[job.b])
                        .map_or(f32::NAN, |echo| echo.alignment.similarity);
                }
            });
        }
    });
    scores
}

/// Per-form AUC over the jobs whose poem satisfies `include`, and their mean.
fn objective(
    scores: &[f32],
    jobs: &[Job],
    poems: &[Poem],
    include: &dyn Fn(usize) -> bool,
) -> (Option<f64>, BTreeMap<String, f64>) {
    let mut by_form: BTreeMap<&str, (Vec<f32>, Vec<f32>)> = BTreeMap::new();
    for (job, &score) in jobs.iter().zip(scores) {
        let Some(p) = job.poem.filter(|&p| include(p)) else {
            continue;
        };
        let entry = by_form.entry(poems[p].form.as_str()).or_default();
        match job.label {
            Label::Echo => entry.0.push(score),
            Label::Not => entry.1.push(score),
            Label::Unlabeled => {}
        }
    }
    let per_form: BTreeMap<String, f64> = by_form
        .into_iter()
        .filter_map(|(form, (pos, neg))| {
            metrics::auc(&pos, &neg).map(|auc| (form.to_string(), auc))
        })
        .collect();
    let mean =
        (!per_form.is_empty()).then(|| per_form.values().sum::<f64>() / per_form.len() as f64);
    (mean, per_form)
}

/// Holds out `fraction` of each form's poems (at least one when a form has
/// two or more), deterministically in `seed`. `true` = held out.
fn holdout(poems: &[Poem], fraction: f64, seed: u64) -> Vec<bool> {
    let mut by_form: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, poem) in poems.iter().enumerate() {
        by_form.entry(&poem.form).or_default().push(i);
    }
    let mut rng = Rng::new(seed ^ 0x5EED_5EA2);
    let mut test = vec![false; poems.len()];
    for indices in by_form.values_mut() {
        indices.sort_by_key(|&i| poems[i].id());
        for k in (1..indices.len()).rev() {
            indices.swap(k, rng.range(0..k + 1));
        }
        let n = indices.len();
        let mut take = (fraction * n as f64).round() as usize;
        if fraction > 0.0 && n >= 2 {
            take = take.clamp(1, n - 1);
        }
        for &i in &indices[..take.min(n)] {
            test[i] = true;
        }
    }
    test
}

struct Trial {
    config: ConfigOverride,
    train: f64,
    test: Option<f64>,
    train_forms: BTreeMap<String, f64>,
    test_forms: BTreeMap<String, f64>,
    fixture_auc: Option<f64>,
}

fn fmt(x: Option<f64>) -> String {
    x.map_or("  —  ".to_string(), |x| format!("{x:.4}"))
}

/// Runs the search for each `--aligner`. `kept` are the scorable pairs
/// (fixture pairs serve only as a reported guard).
pub fn run(
    args: &Args,
    trials: usize,
    poems: &[Poem],
    kept: &[(LabeledPair, usize, usize)],
    lines: &[Line],
) -> Result<(), String> {
    let jobs: Vec<Job> = kept
        .iter()
        .filter(|(pair, ..)| pair.poem.is_some() || pair.source == "fixture")
        .map(|(pair, a, b)| Job {
            a: *a,
            b: *b,
            label: pair.label,
            poem: pair.poem,
        })
        .collect();
    let test = holdout(poems, args.holdout, args.search_seed);
    let train_poems: Vec<String> = poems
        .iter()
        .zip(&test)
        .filter(|(_, t)| !**t)
        .map(|(p, _)| p.id())
        .collect();
    let test_poems: Vec<String> = poems
        .iter()
        .zip(&test)
        .filter(|(_, t)| **t)
        .map(|(p, _)| p.id())
        .collect();
    println!(
        "== search: {} poems for training, {} held out",
        train_poems.len(),
        test_poems.len()
    );
    println!("held out: {}", test_poems.join(", "));
    let pairs_in = |held: bool| {
        jobs.iter()
            .filter(|j| j.poem.is_some_and(|p| test[p] == held) && j.label != Label::Unlabeled)
            .count()
    };
    println!(
        "pairs: {} training, {} held out, {} fixture; {} threads",
        pairs_in(false),
        pairs_in(true),
        jobs.iter().filter(|j| j.poem.is_none()).count(),
        args.threads
    );
    println!();

    std::fs::create_dir_all(repo_path("target/echo")).map_err(|e| e.to_string())?;
    for &kind in &args.aligners {
        let mut rng = Rng::new(args.search_seed.wrapping_add(kind as u64 * 7919));
        let evaluate = |o: &ConfigOverride| -> Result<Trial, String> {
            let mut config = EchoConfig::default();
            o.apply(&mut config)?;
            let detector = EchoDetector::with_config(config);
            let scores = score_all(&detector, kind, &jobs, lines, args.threads);
            let (train, train_forms) = objective(&scores, &jobs, poems, &|p| !test[p]);
            let (test_mean, test_forms) = objective(&scores, &jobs, poems, &|p| test[p]);
            let (fixture_pos, fixture_neg): (Vec<f32>, Vec<f32>) = {
                let pick = |label| -> Vec<f32> {
                    jobs.iter()
                        .zip(&scores)
                        .filter(|(j, _)| j.poem.is_none() && j.label == label)
                        .map(|(_, &s)| s)
                        .collect()
                };
                (pick(Label::Echo), pick(Label::Not))
            };
            Ok(Trial {
                config: o.clone(),
                train: train.unwrap_or(f64::NAN),
                test: test_mean,
                train_forms,
                test_forms,
                fixture_auc: metrics::auc(&fixture_pos, &fixture_neg),
            })
        };

        let start = Instant::now();
        let baseline = evaluate(&override_of(kind, &EchoConfig::default()))?;
        let per_trial = start.elapsed();
        println!(
            "== {kind}: {trials} random + {} refinement trials (~{:.1}s each)",
            trials / 2,
            per_trial.as_secs_f64()
        );
        println!(
            "defaults        train {}  held-out {}  fixture {}",
            fmt(Some(baseline.train)),
            fmt(baseline.test),
            fmt(baseline.fixture_auc)
        );

        let mut tried: Vec<Trial> = Vec::with_capacity(trials + trials / 2 + 1);
        for _ in 0..trials {
            tried.push(evaluate(&sample(kind, &mut rng))?);
        }
        tried.push(baseline);
        let by_train = |tried: &mut Vec<Trial>| {
            tried.sort_by(|a, b| b.train.total_cmp(&a.train));
        };
        by_train(&mut tried);
        for round in 0..trials / 2 {
            let parent = tried[rng.range(0..tried.len().min(5))].config.clone();
            let scale = if round < trials / 4 { 0.15 } else { 0.06 };
            let child = evaluate(&perturb(kind, &parent, scale, &mut rng))?;
            tried.push(child);
            by_train(&mut tried);
        }

        println!(
            "{:<5} {:>8} {:>9} {:>8}  per-form held-out AUC",
            "rank", "train", "held-out", "fixture"
        );
        for (rank, trial) in tried.iter().take(8).enumerate() {
            let forms: Vec<String> = trial
                .test_forms
                .iter()
                .map(|(f, a)| format!("{f} {a:.4}"))
                .collect();
            println!(
                "{:<5} {:>8} {:>9} {:>8}  {}",
                rank + 1,
                fmt(Some(trial.train)),
                fmt(trial.test),
                fmt(trial.fixture_auc),
                forms.join(", ")
            );
        }
        let best = &tried[0];
        let json = serde_json::to_string(&best.config).map_err(|e| e.to_string())?;
        println!("best config:   ECHO_CONFIG='{json}'");
        let train_forms: Vec<String> = best
            .train_forms
            .iter()
            .map(|(f, a)| format!("{f} {a:.4}"))
            .collect();
        println!("best training per-form AUC: {}", train_forms.join(", "));

        // Thresholds at the best config, per form and per strictness, on
        // every poem.
        let mut config = EchoConfig::default();
        best.config.apply(&mut config)?;
        let scores = score_all(
            &EchoDetector::with_config(config),
            kind,
            &jobs,
            lines,
            args.threads,
        );
        let mut groups: BTreeMap<String, (Vec<f32>, Vec<f32>)> = BTreeMap::new();
        for (job, &score) in jobs.iter().zip(&scores) {
            let Some(p) = job.poem else { continue };
            for key in [
                format!("form {}", poems[p].form),
                format!("strictness {}", poems[p].strictness),
            ] {
                let entry = groups.entry(key).or_default();
                match job.label {
                    Label::Echo => entry.0.push(score),
                    Label::Not => entry.1.push(score),
                    Label::Unlabeled => {}
                }
            }
        }
        for (group, (pos, neg)) in &groups {
            if let Some(op) = metrics::best_threshold(pos, neg) {
                println!(
                    "  {group:<20} threshold {:.3}  BA {:.3}  TPR {:.3}  TNR {:.3}  AUC {}",
                    op.threshold,
                    op.balanced_accuracy,
                    op.tpr,
                    op.tnr,
                    fmt(metrics::auc(pos, neg))
                );
            }
        }

        let path = repo_path(&format!(
            "target/echo/search-{kind}-seed{}.jsonl",
            args.search_seed
        ));
        let mut file =
            std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for trial in &tried {
            let row = serde_json::json!({
                "config": trial.config,
                "train": trial.train,
                "held_out": trial.test,
                "train_forms": trial.train_forms,
                "held_out_forms": trial.test_forms,
                "fixture_auc": trial.fixture_auc,
            });
            writeln!(file, "{row}").map_err(|e| e.to_string())?;
        }
        println!(
            "all {} trials in {}  ({:.0}s)",
            tried.len(),
            path.display(),
            start.elapsed().as_secs_f64()
        );
        println!();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_stay_in_range_and_coherent() {
        let mut rng = Rng::new(3);
        for kind in AlignerKind::ALL {
            for _ in 0..200 {
                let mut o = perturb(kind, &sample(kind, &mut rng), 0.3, &mut rng);
                for p in params(kind) {
                    let v = slot(&mut o, p.name).expect("every searched parameter is set");
                    assert!((p.lo..=p.hi).contains(&v), "{} = {v}", p.name);
                }
                assert!(o.unstressed_weight <= o.secondary_weight);
                assert!(o.gap_open_repeated_consonant <= o.gap_open);
                let mut config = EchoConfig::default();
                o.apply(&mut config).unwrap();
            }
        }
        // The semi-global aligner ignores structure costs, so they aren't searched.
        assert!(
            sample(AlignerKind::SemiGlobal, &mut rng)
                .move_cost
                .is_none()
        );
    }

    #[test]
    fn defaults_round_trip_through_an_override() {
        for kind in AlignerKind::ALL {
            let mut config = EchoConfig::default();
            override_of(kind, &EchoConfig::default())
                .apply(&mut config)
                .unwrap();
            assert_eq!(config, EchoConfig::default());
        }
    }
}
