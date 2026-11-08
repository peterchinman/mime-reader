//! Parameter search ("fuzzing") over echo configs.
//!
//! Each candidate config scores every pair. Its objective is the mean AUC
//! (echo vs non-echo) over groups — strict and loose poems by default, or
//! forms — so a large group doesn't drown a small one; within a group,
//! each poem counts by its number of pairs.
//!
//! Random sampling over the parameters that matter to the aligner comes
//! first — each sample screened on one fold and fully scored only if it is
//! within reach of the best so far — then rounds of local perturbation
//! around the best few configs. Every config is applied on top of the base:
//! the aligner's tuned config plus any `--config` / `ECHO_CONFIG` override.
//!
//! Poems are split into k folds, stratified by form and strictness. Every
//! trial records its objective on each fold and on everything but each
//! fold, at no extra scoring cost, which gives a cross-validation estimate:
//! for each fold, the trial that would have won without it, judged on it.
//! To keep that honest, it only draws on the random trials (refinement
//! picks parents by the all-poem objective, which has seen every fold) and
//! skips the screen fold (the random trials were filtered on it). The base
//! is left out too: it may itself have been tuned on these poems. Per-fold
//! and all-poem objectives of single configs are in-sample.
//!
//! `--search 0` scores only the base, per fold: a way to compare fixed
//! configs without any selection.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use mime_reader::echo::{AlignerKind, EchoConfig, EchoDetector, EchoNormalization};
use mime_reader::line::Line;

use crate::args::{Args, ConfigOverride, Objective, repo_path};
use crate::corpus::{Label, LabeledPair, Poem};
use crate::metrics;
use crate::mutate::Rng;

/// A searchable integer parameter: its name, its range, and where it lives
/// in a [`ConfigOverride`] and an [`EchoConfig`].
struct Param {
    #[allow(dead_code)] // named in test failures
    name: &'static str,
    lo: u32,
    hi: u32,
    slot: fn(&mut ConfigOverride) -> &mut Option<u32>,
    get: fn(&EchoConfig) -> u32,
}

/// `param!(field, lo, hi, |config| value)`: the override field and the
/// parameter share a name.
macro_rules! param {
    ($field:ident, $lo:expr, $hi:expr, |$c:ident| $get:expr) => {
        Param {
            name: stringify!($field),
            lo: $lo,
            hi: $hi,
            slot: |o| &mut o.$field,
            get: |$c| $get,
        }
    };
}

const SIMILARITY: [Param; 4] = [
    param!(vowel_coefficient, 1, 8, |c| c.similarity.vowel_coefficient),
    param!(consonant_coefficient, 1, 8, |c| c
        .similarity
        .consonant_coefficient),
    param!(vowel_zero_point, 150, 700, |c| c
        .similarity
        .vowel_zero_point),
    param!(consonant_zero_point, 100, 1000, |c| c
        .similarity
        .consonant_zero_point),
];
const STRESS: [Param; 2] = [
    param!(secondary_weight, 1, 4, |c| c
        .similarity
        .stress_weights
        .secondary),
    param!(unstressed_weight, 1, 4, |c| c
        .similarity
        .stress_weights
        .unstressed),
];
const GAPS: [Param; 3] = [
    param!(gap_open, 0, 1500, |c| c.gaps.gap_open),
    param!(gap_extend, 0, 200, |c| c.gaps.gap_extend),
    param!(gap_open_repeated_consonant, 0, 1500, |c| c
        .gaps
        .gap_open_repeated_consonant),
];
const RUNS: [Param; 3] = [
    param!(move_cost, 0, 1500, |c| c.structure.move_cost),
    param!(run_open, 0, 1500, |c| c.structure.run_open),
    param!(min_run, 200, 1500, |c| c.structure.min_run),
];

/// The parameters that affect `kind`.
fn params(kind: AlignerKind) -> Vec<&'static Param> {
    let mut all: Vec<&Param> = SIMILARITY.iter().chain(&STRESS).chain(&GAPS).collect();
    match kind {
        AlignerKind::SemiGlobal => {}
        AlignerKind::WatermanEggert => all.extend(&RUNS),
    }
    all
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
        *(p.slot)(&mut o) = Some(rng.range(p.lo as usize..p.hi as usize + 1) as u32);
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
        let current = f64::from((p.slot)(&mut o).unwrap_or(p.lo));
        let step = (uniform(rng) * 2.0 - 1.0) * scale * span;
        let next = (current + step)
            .round()
            .clamp(f64::from(p.lo), f64::from(p.hi));
        *(p.slot)(&mut o) = Some(next as u32);
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
    for p in params(kind) {
        *(p.slot)(&mut o) = Some((p.get)(config));
    }
    o.normalization = Some(
        match config.normalization {
            EchoNormalization::ShorterSelfSimilarity => "shorter",
            EchoNormalization::MeanSelfSimilarity => "mean",
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

/// Scores every job under `detector`, spread over `threads`. Threads take
/// small batches from a shared counter, so faster cores (or cores that drew
/// easy pairs) keep working instead of idling at the end. Jobs must be
/// scorable: pairs with unknown words or empty lines are dropped upstream.
pub fn score_all(
    detector: &EchoDetector,
    jobs: &[Job],
    lines: &[Line],
    threads: usize,
) -> Vec<f32> {
    const BATCH: usize = 32;
    let next = AtomicUsize::new(0);
    let mut scores = vec![0.0f32; jobs.len()];
    let batches: Vec<Vec<(usize, Vec<f32>)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let start = next.fetch_add(BATCH, Ordering::Relaxed);
                        if start >= jobs.len() {
                            break done;
                        }
                        let end = (start + BATCH).min(jobs.len());
                        let batch = jobs[start..end]
                            .iter()
                            .map(|job| {
                                detector
                                    .compare_lines(&lines[job.a], &lines[job.b])
                                    .expect("unscorable pairs are dropped before scoring")
                                    .alignment
                                    .similarity
                            })
                            .collect();
                        done.push((start, batch));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().expect("a scoring thread panicked"))
            .collect()
    });
    for (start, batch) in batches.into_iter().flatten() {
        scores[start..start + batch.len()].copy_from_slice(&batch);
    }
    scores
}

/// The reporting group a poem falls in under `objective`.
fn group_of(poem: &Poem, objective: Objective) -> &str {
    match objective {
        Objective::Strictness => &poem.strictness,
        Objective::Form => &poem.form,
    }
}

/// AUC per group (strictness or form) over the jobs whose poem satisfies
/// `include`, and their mean.
fn objective(
    scores: &[f32],
    jobs: &[Job],
    poems: &[Poem],
    include: &dyn Fn(usize) -> bool,
    by: Objective,
) -> (Option<f64>, BTreeMap<String, f64>) {
    let mut groups: BTreeMap<&str, (Vec<f32>, Vec<f32>)> = BTreeMap::new();
    for (job, &score) in jobs.iter().zip(scores) {
        let Some(p) = job.poem.filter(|&p| include(p)) else {
            continue;
        };
        let entry = groups.entry(group_of(&poems[p], by)).or_default();
        match job.label {
            Label::Echo => entry.0.push(score),
            Label::Not => entry.1.push(score),
            Label::Unlabeled => {}
        }
    }
    let per_group: BTreeMap<String, f64> = groups
        .into_iter()
        .filter_map(|(group, (pos, neg))| {
            metrics::auc(&pos, &neg).map(|auc| (group.to_string(), auc))
        })
        .collect();
    let mean =
        (!per_group.is_empty()).then(|| per_group.values().sum::<f64>() / per_group.len() as f64);
    (mean, per_group)
}

/// Assigns each poem to one of `k` folds, round-robin within each (form,
/// strictness) stratum after a seeded shuffle, so every fold gets a fair
/// share of each kind of poem.
fn assign_folds(poems: &[Poem], k: usize, seed: u64) -> Vec<usize> {
    let mut strata: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, poem) in poems.iter().enumerate() {
        strata
            .entry((&poem.form, &poem.strictness))
            .or_default()
            .push(i);
    }
    let mut rng = Rng::new(seed ^ 0x5EED_5EA2);
    let mut fold = vec![0; poems.len()];
    let mut offset = 0;
    for indices in strata.values_mut() {
        indices.sort_by_key(|&i| poems[i].id());
        for x in (1..indices.len()).rev() {
            indices.swap(x, rng.range(0..x + 1));
        }
        for (n, &i) in indices.iter().enumerate() {
            fold[i] = (offset + n) % k;
        }
        // Rotate where each stratum starts, so small strata don't all pile
        // into fold 0.
        offset += indices.len();
    }
    fold
}

/// How a trial's config was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The base config.
    Base,
    /// Drawn at random (and through the screen).
    Random,
    /// Perturbed from one of the best so far.
    Refine,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Random => "random",
            Self::Refine => "refine",
        }
    }
}

struct Trial {
    config: ConfigOverride,
    phase: Phase,
    /// The objective on every poem: what the search maximizes.
    all: f64,
    all_groups: BTreeMap<String, f64>,
    /// Per-form AUC on every poem, for reporting.
    forms: BTreeMap<String, f64>,
    /// `folds[k]`: the objective on fold `k`'s poems alone.
    folds: Vec<Option<f64>>,
    /// `without[k]`: the objective on every poem except fold `k`'s.
    without: Vec<Option<f64>>,
    fixture_auc: Option<f64>,
}

fn fmt(x: Option<f64>) -> String {
    x.map_or("  —  ".to_string(), |x| format!("{x:.4}"))
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let xs: Vec<f64> = xs.collect();
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

/// The honest estimate of the search: for each fold except `screen_fold`,
/// the random trial that scores best *without* that fold, judged on that
/// fold; averaged over those folds (see the module docs).
fn nested_estimate(
    tried: &[Trial],
    k: usize,
    screen_fold: usize,
) -> (Option<f64>, Vec<Option<f64>>) {
    let per_fold: Vec<Option<f64>> = (0..k)
        .map(|f| {
            if f == screen_fold {
                return None;
            }
            tried
                .iter()
                .filter(|t| t.phase == Phase::Random && t.without[f].is_some())
                .max_by(|a, b| a.without[f].unwrap().total_cmp(&b.without[f].unwrap()))
                .and_then(|t| t.folds[f])
        })
        .collect();
    (mean(per_fold.iter().flatten().copied()), per_fold)
}

/// The mean over every fold but `screen_fold`: the folds the estimate uses.
fn mean_outside(folds: &[Option<f64>], screen_fold: usize) -> Option<f64> {
    mean(
        folds
            .iter()
            .enumerate()
            .filter(|&(f, _)| f != screen_fold)
            .filter_map(|(_, x)| *x),
    )
}

/// Runs the search for each `--aligner`, from its base config in `bases`
/// (same order). `kept` are the scorable pairs (fixture pairs serve only as
/// a reported guard).
pub fn run(
    args: &Args,
    trials: usize,
    bases: &[EchoConfig],
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
    let k = args.folds;
    let fold = assign_folds(poems, k, args.search_seed);
    // Random configs are first scored on one fold (the screen) and only
    // fully scored if they come close to the best screen score so far.
    const SCREEN_FOLD: usize = 0;
    const SCREEN_MARGIN: f64 = 0.01;
    let screen_jobs: Vec<Job> = jobs
        .iter()
        .filter(|j| j.poem.is_some_and(|p| fold[p] == SCREEN_FOLD))
        .copied()
        .collect();

    println!(
        "== search: {} poems in {k} folds (stratified by form and strictness); objective = mean AUC by {}",
        poems.len(),
        match args.objective {
            Objective::Strictness => "strictness (strict, loose)",
            Objective::Form => "form",
        }
    );
    for f in 0..k {
        let in_fold: Vec<&Poem> = poems
            .iter()
            .zip(&fold)
            .filter(|(_, x)| **x == f)
            .map(|(p, _)| p)
            .collect();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for p in &in_fold {
            *counts
                .entry(format!("{} {}", p.strictness, p.form))
                .or_default() += 1;
        }
        let summary: Vec<String> = counts.iter().map(|(g, n)| format!("{g} {n}")).collect();
        println!(
            "  fold {f}: {} poems ({})",
            in_fold.len(),
            summary.join(", ")
        );
    }
    println!(
        "pairs: {} labeled, {} in the screen fold, {} fixture; {} threads",
        jobs.iter()
            .filter(|j| j.poem.is_some() && j.label != Label::Unlabeled)
            .count(),
        screen_jobs
            .iter()
            .filter(|j| j.label != Label::Unlabeled)
            .count(),
        jobs.iter().filter(|j| j.poem.is_none()).count(),
        args.threads
    );
    println!();

    std::fs::create_dir_all(repo_path("target/echo")).map_err(|e| e.to_string())?;
    for (&kind, &base) in args.aligners.iter().zip(bases) {
        let mut rng = Rng::new(args.search_seed.wrapping_add(kind as u64 * 7919));
        let detector_for = |o: &ConfigOverride| -> Result<EchoDetector, String> {
            let mut config = base;
            o.apply(&mut config)?;
            Ok(EchoDetector::with_config(config))
        };
        let screen = |o: &ConfigOverride| -> Result<f64, String> {
            let scores = score_all(&detector_for(o)?, &screen_jobs, lines, args.threads);
            Ok(
                objective(&scores, &screen_jobs, poems, &|_| true, args.objective)
                    .0
                    .unwrap_or(f64::NAN),
            )
        };
        let evaluate = |o: &ConfigOverride, phase: Phase| -> Result<(Trial, f64), String> {
            let scores = score_all(&detector_for(o)?, &jobs, lines, args.threads);
            let (all, all_groups) = objective(&scores, &jobs, poems, &|_| true, args.objective);
            let (_, forms) = objective(&scores, &jobs, poems, &|_| true, Objective::Form);
            let folds = (0..k)
                .map(|f| objective(&scores, &jobs, poems, &|p| fold[p] == f, args.objective).0)
                .collect::<Vec<_>>();
            let without = (0..k)
                .map(|f| objective(&scores, &jobs, poems, &|p| fold[p] != f, args.objective).0)
                .collect();
            let pick = |label| -> Vec<f32> {
                jobs.iter()
                    .zip(&scores)
                    .filter(|(j, _)| j.poem.is_none() && j.label == label)
                    .map(|(_, &s)| s)
                    .collect()
            };
            let screen_score = folds[SCREEN_FOLD].unwrap_or(f64::NAN);
            Ok((
                Trial {
                    config: o.clone(),
                    phase,
                    all: all.unwrap_or(f64::NAN),
                    all_groups,
                    forms,
                    folds,
                    without,
                    fixture_auc: metrics::auc(&pick(Label::Echo), &pick(Label::Not)),
                },
                screen_score,
            ))
        };

        let start = Instant::now();
        let (baseline, baseline_screen) = evaluate(&override_of(kind, &base), Phase::Base)?;
        let per_trial = start.elapsed();
        println!(
            "== {kind}: {trials} random (screened on fold {SCREEN_FOLD}) + {} refinement trials (~{:.1}s per full trial)",
            trials / 2,
            per_trial.as_secs_f64()
        );
        let per_fold: Vec<String> = baseline.folds.iter().map(|x| fmt(*x)).collect();
        println!(
            "base            objective {}  per fold [{}]  fixture {}  (in-sample)",
            fmt(Some(baseline.all)),
            per_fold.join(", "),
            fmt(baseline.fixture_auc)
        );

        let mut best_screen = baseline_screen;
        let mut screened_out = 0;
        let mut tried: Vec<Trial> = Vec::with_capacity(trials + trials / 2 + 1);
        tried.push(baseline);
        for _ in 0..trials {
            let candidate = sample(kind, &mut rng);
            let quick = screen(&candidate)?;
            if quick.is_nan() || quick < best_screen - SCREEN_MARGIN {
                screened_out += 1;
                continue;
            }
            let (trial, screen_score) = evaluate(&candidate, Phase::Random)?;
            best_screen = best_screen.max(screen_score);
            tried.push(trial);
        }
        let by_objective = |tried: &mut Vec<Trial>| {
            tried.sort_by(|a, b| b.all.total_cmp(&a.all));
        };
        by_objective(&mut tried);
        for round in 0..trials / 2 {
            let parent = tried[rng.range(0..tried.len().min(5))].config.clone();
            let scale = if round < trials / 4 { 0.15 } else { 0.06 };
            let (child, _) = evaluate(&perturb(kind, &parent, scale, &mut rng), Phase::Refine)?;
            tried.push(child);
            by_objective(&mut tried);
        }

        println!(
            "screened out {screened_out} of {trials} random configs; {} fully scored",
            tried.len()
        );
        println!(
            "{:<5} {:<7} {:>9} {:>9} {:>8}  per-group AUC | per-form AUC (all poems; in-sample)",
            "rank", "phase", "objective", "mean fold", "fixture"
        );
        for (rank, trial) in tried.iter().take(8).enumerate() {
            let groups: Vec<String> = trial
                .all_groups
                .iter()
                .map(|(g, a)| format!("{g} {a:.4}"))
                .collect();
            let forms: Vec<String> = trial
                .forms
                .iter()
                .map(|(f, a)| format!("{f} {a:.4}"))
                .collect();
            println!(
                "{:<5} {:<7} {:>9} {:>9} {:>8}  {} | {}",
                rank + 1,
                trial.phase.name(),
                fmt(Some(trial.all)),
                fmt(mean(trial.folds.iter().flatten().copied())),
                fmt(trial.fixture_auc),
                groups.join(", "),
                forms.join(", ")
            );
        }
        let (nested, per_fold) = nested_estimate(&tried, k, SCREEN_FOLD);
        let base_trial = tried
            .iter()
            .find(|t| t.phase == Phase::Base)
            .expect("the base is among the trials");
        let per_fold: Vec<String> = per_fold.iter().map(|x| fmt(*x)).collect();
        println!(
            "CV estimate (best random config chosen without each fold, scored on it; screen fold {SCREEN_FOLD} skipped): {} [{}]; base on the same folds {}",
            fmt(nested),
            per_fold.join(", "),
            fmt(mean_outside(&base_trial.folds, SCREEN_FOLD))
        );
        let best = &tried[0];
        let json = serde_json::to_string(&best.config).map_err(|e| e.to_string())?;
        println!("best config:   ECHO_CONFIG='{json}'");

        // Thresholds at the best config, per form and per strictness, on
        // every poem.
        let scores = score_all(&detector_for(&best.config)?, &jobs, lines, args.threads);
        let mut groups: BTreeMap<String, (Vec<f32>, Vec<f32>)> = BTreeMap::new();
        for (job, &score) in jobs.iter().zip(&scores) {
            let Some(p) = job.poem else { continue };
            for key in [
                format!("form {}", poems[p].form),
                format!("strictness {}", poems[p].strictness),
                format!("{} {}", poems[p].strictness, poems[p].form),
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
                    "  {group:<24} threshold {:.3}  BA {:.3}  TPR {:.3}  TNR {:.3}  AUC {}",
                    op.threshold,
                    op.balanced_accuracy,
                    op.tpr,
                    op.tnr,
                    fmt(metrics::auc(pos, neg))
                );
            }
        }

        // Named by trial count too, so a quick `--search 0` can't overwrite
        // a real search's log.
        let path = repo_path(&format!(
            "target/echo/search-{kind}-seed{}-n{trials}.jsonl",
            args.search_seed
        ));
        let mut file =
            std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for trial in &tried {
            let row = serde_json::json!({
                "config": trial.config,
                "phase": trial.phase.name(),
                "objective": trial.all,
                "groups": trial.all_groups,
                "forms": trial.forms,
                "folds": trial.folds,
                "without_fold": trial.without,
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
                    let v = (p.slot)(&mut o).expect("every searched parameter is set");
                    assert!((p.lo..=p.hi).contains(&v), "{} = {v}", p.name);
                }
                assert!(o.unstressed_weight <= o.secondary_weight);
                assert!(o.gap_open_repeated_consonant <= o.gap_open);
                let mut config = EchoConfig::for_aligner(kind);
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
    fn the_estimate_uses_only_random_trials_outside_the_screen_fold() {
        // Three folds; fold 0 is the screen.
        let trial = |phase, folds: [f64; 3], without: [f64; 3]| Trial {
            config: ConfigOverride::default(),
            phase,
            all: 0.0,
            all_groups: BTreeMap::new(),
            forms: BTreeMap::new(),
            folds: folds.map(Some).to_vec(),
            without: without.map(Some).to_vec(),
            fixture_auc: None,
        };
        let tried = [
            // The base and a refinement look best everywhere: ignored.
            trial(Phase::Base, [0.99; 3], [0.99; 3]),
            trial(Phase::Refine, [0.98; 3], [0.98; 3]),
            // Random trials: the second wins without fold 1, the first
            // without fold 2.
            trial(Phase::Random, [0.5, 0.6, 0.7], [0.9, 0.1, 0.9]),
            trial(Phase::Random, [0.4, 0.8, 0.3], [0.1, 0.9, 0.1]),
        ];
        let (estimate, per_fold) = nested_estimate(&tried, 3, 0);
        assert_eq!(per_fold, [None, Some(0.8), Some(0.7)]);
        assert!((estimate.unwrap() - 0.75).abs() < 1e-12);
        assert_eq!(mean_outside(&tried[0].folds, 0), Some(0.99));
    }

    #[test]
    fn tuned_configs_round_trip_through_an_override() {
        for kind in AlignerKind::ALL {
            let tuned = EchoConfig::for_aligner(kind);
            // Applied to the other aligner's config, an override still
            // carries everything `kind` searches.
            let mut config = EchoConfig::default();
            override_of(kind, &tuned).apply(&mut config).unwrap();
            assert_eq!(override_of(kind, &config), override_of(kind, &tuned));
            let mut config = tuned;
            override_of(kind, &tuned).apply(&mut config).unwrap();
            assert_eq!(config, tuned);
        }
    }

    #[test]
    fn folds_spread_every_stratum() {
        use crate::corpus::parse_poem;
        use std::path::Path;
        let poem = |form: &str, strictness: &str, n: usize| {
            let text = format!(
                "form: refrain\ntitle: {form}{n}\nauthor: A\nstrictness: {strictness}\ntemplate: A_A\n\nx\ny\nx\n"
            );
            let mut p =
                parse_poem(&text, Path::new(&format!("{form}-{n}.txt")), "private").unwrap();
            p.form = form.to_string();
            p
        };
        let mut poems = Vec::new();
        for n in 0..7 {
            poems.push(poem("pantoum", "loose", n));
        }
        for n in 0..3 {
            poems.push(poem("duplex", "loose", n));
        }
        let fold = assign_folds(&poems, 5, 1);
        assert!(fold.iter().all(|&f| f < 5));
        // Seven loose pantoums over five folds: every fold gets one or two.
        let mut per_fold = [0; 5];
        for (p, &f) in poems.iter().zip(&fold) {
            if p.form == "pantoum" {
                per_fold[f] += 1;
            }
        }
        assert!(
            per_fold.iter().all(|&n| (1..=2).contains(&n)),
            "{per_fold:?}"
        );
        // The three duplexes land in three different folds.
        let mut duplex_folds: Vec<usize> = poems
            .iter()
            .zip(&fold)
            .filter(|(p, _)| p.form == "duplex")
            .map(|(_, &f)| f)
            .collect();
        duplex_folds.sort();
        duplex_folds.dedup();
        assert_eq!(duplex_folds.len(), 3);
        assert_eq!(
            assign_folds(&poems, 5, 1),
            fold,
            "deterministic in the seed"
        );
    }
}
