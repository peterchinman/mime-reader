//! The printed report.

use std::collections::BTreeMap;

use mime_reader::echo::{AlignerKind, EchoConfig, EchoDetector, EchoStrictness};

use crate::Scored;
use crate::corpus::{Label, Poem};
use crate::metrics::{self, Operating};

pub fn config(kind: AlignerKind, config: &EchoConfig, detector: &EchoDetector) {
    let s = &config.similarity;
    let g = &config.gaps;
    let t = &config.structure;
    println!("== {kind}: effective echo config (costs in thousandths of a unit)");
    println!(
        "similarity: vowel×{} consonant×{}  zero points vowel {} consonant {}  stress weights {}/{}/{}",
        s.vowel_coefficient,
        s.consonant_coefficient,
        s.vowel_zero_point,
        s.consonant_zero_point,
        s.stress_weights.primary,
        s.stress_weights.secondary,
        s.stress_weights.unstressed
    );
    println!(
        "gaps: open {} extend {} open_repeated_consonant {}",
        g.gap_open, g.gap_extend, g.gap_open_repeated_consonant
    );
    println!(
        "structure: move {} run_open {} min_run {} max_runs {}",
        t.move_cost, t.run_open, t.min_run, t.max_runs
    );
    println!(
        "normalization {:?}  variants ≤{}/line, ≤{} pairs  unit = {}",
        config.normalization,
        config.max_variants_per_line,
        config.max_variant_pairs,
        detector.unit()
    );
    println!();
}

fn cell(xs: &[f32]) -> String {
    match metrics::summary(xs) {
        Some(s) => format!("{:6.3} ±{:5.3} [{:6.3},{:6.3}]", s.mean, s.sd, s.min, s.max),
        None => format!("{:^32}", "—"),
    }
}

/// Per (source, group): n and each aligner's mean ± sd [min, max].
pub fn groups(scored: &[Scored], aligners: &[AlignerKind]) {
    println!("== similarity by source and group");
    print!("{:<34} {:>5}", "source / group", "n");
    for kind in aligners {
        print!("  {:^32}", kind.to_string());
    }
    println!();
    let mut by_group: BTreeMap<(u8, &str, &str), Vec<&Scored>> = BTreeMap::new();
    for row in scored {
        let order = match row.pair.source.as_str() {
            "fixture" => 0,
            "public" => 1,
            "private" => 2,
            _ => 3,
        };
        by_group
            .entry((order, &row.pair.source, &row.pair.group))
            .or_default()
            .push(row);
    }
    for ((_, source, group), rows) in by_group {
        print!("{:<34} {:>5}", format!("{source} / {group}"), rows.len());
        for k in 0..aligners.len() {
            let xs: Vec<f32> = rows.iter().map(|r| r.scores[k]).collect();
            print!("  {}", cell(&xs));
        }
        println!();
    }
    println!();
}

/// A named evaluation set: which rows count as positives and negatives.
pub struct EvalSet<'s> {
    pub name: &'static str,
    pub positives: Vec<&'s Scored>,
    pub negatives: Vec<&'s Scored>,
}

pub fn eval_sets(scored: &[Scored]) -> Vec<EvalSet<'_>> {
    let is_corpus = |r: &&Scored| r.pair.source == "public" || r.pair.source == "private";
    let pick = |source: &dyn Fn(&&Scored) -> bool, label: Label| -> Vec<&Scored> {
        scored
            .iter()
            .filter(|r| source(r) && r.pair.label == label)
            .collect()
    };
    let fixture = |r: &&Scored| r.pair.source == "fixture";
    let mutation = |r: &&Scored| r.pair.source == "mutation";
    let mut corpus_and_mutations = pick(&is_corpus, Label::Echo);
    corpus_and_mutations.extend(pick(&mutation, Label::Echo));
    let mut sets = vec![
        EvalSet {
            name: "fixture",
            positives: pick(&fixture, Label::Echo),
            negatives: pick(&fixture, Label::Not),
        },
        EvalSet {
            name: "corpus",
            positives: pick(&is_corpus, Label::Echo),
            negatives: pick(&is_corpus, Label::Not),
        },
        EvalSet {
            name: "corpus+mutations",
            positives: corpus_and_mutations,
            negatives: pick(&is_corpus, Label::Not),
        },
    ];
    for source in ["public", "private"] {
        let from = move |r: &&Scored| r.pair.source == source;
        let set = EvalSet {
            name: source,
            positives: pick(&from, Label::Echo),
            negatives: pick(&from, Label::Not),
        };
        if !set.positives.is_empty() {
            sets.push(set);
        }
    }
    sets.push(EvalSet {
        name: "all",
        positives: scored
            .iter()
            .filter(|r| r.pair.label == Label::Echo)
            .collect(),
        negatives: scored
            .iter()
            .filter(|r| r.pair.label == Label::Not)
            .collect(),
    });
    sets.retain(|set| !set.positives.is_empty() && !set.negatives.is_empty());
    sets
}

fn column(rows: &[&Scored], k: usize) -> Vec<f32> {
    rows.iter().map(|r| r.scores[k]).collect()
}

/// AUC, best threshold, balanced accuracy, F1 and margin per aligner and set.
pub fn separation(
    sets: &[EvalSet<'_>],
    aligners: &[AlignerKind],
) -> BTreeMap<(&'static str, usize), Operating> {
    println!("== separation (echo if similarity ≥ threshold)");
    println!(
        "{:<17} {:<16} {:>6} {:>6} {:>7} {:>9} {:>6} {:>6} {:>6} {:>6} {:>8}",
        "set", "aligner", "pos", "neg", "AUC", "threshold", "BA", "TPR", "TNR", "F1", "margin"
    );
    let mut best = BTreeMap::new();
    for set in sets {
        for (k, kind) in aligners.iter().enumerate() {
            let (p, n) = (column(&set.positives, k), column(&set.negatives, k));
            let auc = metrics::auc(&p, &n);
            let operating = metrics::best_threshold(&p, &n);
            let margin = metrics::margin(&p, &n);
            let fmt = |x: Option<f64>, w: usize| {
                x.map_or(format!("{:>w$}", "—"), |x| format!("{x:>w$.3}"))
            };
            println!(
                "{:<17} {:<16} {:>6} {:>6} {} {} {} {} {} {} {}",
                set.name,
                kind.to_string(),
                p.len(),
                n.len(),
                fmt(auc, 7),
                fmt(operating.map(|o| f64::from(o.threshold)), 9),
                fmt(operating.map(|o| o.balanced_accuracy), 6),
                fmt(operating.map(|o| o.tpr), 6),
                fmt(operating.map(|o| o.tnr), 6),
                fmt(operating.map(|o| o.f1), 6),
                fmt(margin.map(f64::from), 8),
            );
            if let Some(operating) = operating {
                best.insert((set.name, k), operating);
            }
        }
    }
    println!();
    best
}

/// Pass rates at [`EchoStrictness::threshold`]: corpus poems pooled by
/// their strictness, and the fixture (judged at both). The thresholds were
/// measured under the tuned configs, so an override makes these
/// indicative only.
pub fn shipped_thresholds(
    scored: &[Scored],
    poems: &[Poem],
    aligners: &[AlignerKind],
    overridden: bool,
) {
    println!(
        "== at the shipped thresholds (echoes caught / non-echoes rejected){}",
        if overridden {
            " — config overridden: thresholds measured without the override"
        } else {
            ""
        }
    );
    for (k, kind) in aligners.iter().enumerate() {
        for strictness in EchoStrictness::ALL {
            let threshold = strictness.threshold(*kind);
            let rates = |pool: &dyn Fn(&Scored) -> bool| {
                let (mut pos, mut neg) = (Vec::new(), Vec::new());
                for row in scored.iter().filter(|r| pool(r)) {
                    match row.pair.label {
                        Label::Echo => pos.push(row.scores[k]),
                        Label::Not => neg.push(row.scores[k]),
                        Label::Unlabeled => {}
                    }
                }
                if pos.is_empty() || neg.is_empty() {
                    return format!("{:^28}", "—");
                }
                let op = metrics::at_threshold(&pos, &neg, threshold);
                format!(
                    "{:.3} / {:.3} ({} / {})",
                    op.tpr,
                    op.tnr,
                    pos.len(),
                    neg.len()
                )
            };
            let corpus = rates(&|r: &Scored| {
                r.pair
                    .poem
                    .is_some_and(|p| poems[p].strictness == strictness.name())
            });
            let fixture = rates(&|r: &Scored| r.pair.source == "fixture");
            println!(
                "{:<16} {:<7} {threshold:.2}  {strictness} poems {corpus}  fixture {fixture}",
                kind.to_string(),
                strictness.name(),
            );
        }
    }
    println!();
}

/// Each set's best threshold applied to the others.
pub fn transfer(
    sets: &[EvalSet<'_>],
    best: &BTreeMap<(&'static str, usize), Operating>,
    aligners: &[AlignerKind],
) {
    println!(
        "== threshold transfer (balanced accuracy when one set's best threshold is used on another)"
    );
    for (k, kind) in aligners.iter().enumerate() {
        for (from, to) in [
            ("fixture", "corpus"),
            ("corpus", "fixture"),
            ("fixture", "corpus+mutations"),
        ] {
            let (Some(source), Some(target)) =
                (best.get(&(from, k)), sets.iter().find(|s| s.name == to))
            else {
                continue;
            };
            let Some(own) = best.get(&(to, k)) else {
                continue;
            };
            let applied = metrics::at_threshold(
                &column(&target.positives, k),
                &column(&target.negatives, k),
                source.threshold,
            );
            println!(
                "{:<16} {from:>8} → {to:<17} threshold {:.3} (own {:.3}, Δ {:+.3}): BA {:.3} (own {:.3})",
                kind.to_string(),
                source.threshold,
                own.threshold,
                source.threshold - own.threshold,
                applied.balanced_accuracy,
                own.balanced_accuracy
            );
        }
    }
    println!();
}

/// Per-poem ranking: precision@k, MRR, perfectly separated poems.
pub fn poems(scored: &[Scored], poems: &[Poem], aligners: &[AlignerKind], by_poem: bool) {
    let mut per_poem: BTreeMap<usize, Vec<&Scored>> = BTreeMap::new();
    for row in scored {
        if let Some(p) = row.pair.poem {
            per_poem.entry(p).or_default().push(row);
        }
    }
    if per_poem.is_empty() {
        return;
    }
    println!("== per-poem ranking (precision@k / MRR / separated)");
    let ranks: Vec<(usize, Vec<Option<metrics::PoemRank>>)> = per_poem
        .iter()
        .map(|(&p, rows)| {
            let ranks = (0..aligners.len())
                .map(|k| {
                    let pairs: Vec<(usize, usize, Label, f32)> = rows
                        .iter()
                        .filter_map(|r| {
                            r.pair.lines.map(|(i, j)| (i, j, r.pair.label, r.scores[k]))
                        })
                        .collect();
                    metrics::poem_rank(&pairs)
                })
                .collect();
            (p, ranks)
        })
        .collect();
    if by_poem {
        print!("{:<48} {:>4}", "poem", "k");
        for kind in aligners {
            print!("  {:^20}", kind.to_string());
        }
        println!();
        for (p, row) in &ranks {
            let k = row.iter().flatten().next().map_or(0, |r| r.k);
            print!("{:<48} {k:>4}", poems[*p].id());
            for rank in row {
                match rank {
                    Some(r) => print!(
                        "  {:.2} / {:.2} / {:<5}",
                        r.precision_at_k,
                        r.mrr,
                        if r.separated { "yes" } else { "no" }
                    ),
                    None => print!("  {:^20}", "—"),
                }
            }
            println!();
            println!("    {}", poems[*p].describe());
        }
    }
    for (k, kind) in aligners.iter().enumerate() {
        let all: Vec<&metrics::PoemRank> =
            ranks.iter().filter_map(|(_, r)| r[k].as_ref()).collect();
        if all.is_empty() {
            continue;
        }
        let n = all.len() as f64;
        println!(
            "{:<16} poems {:>3}  mean precision@k {:.3}  MRR {:.3}  perfectly separated {}/{}",
            kind.to_string(),
            all.len(),
            all.iter().map(|r| r.precision_at_k).sum::<f64>() / n,
            all.iter().map(|r| r.mrr).sum::<f64>() / n,
            all.iter().filter(|r| r.separated).count(),
            all.len()
        );
    }
    println!();
}

/// The labeled pairs on which the aligners disagree most, from the
/// classes where they are expected to: insertions, moves,
/// resegmentation, and duplexes.
pub fn disagreements(scored: &[Scored], aligners: &[AlignerKind], limit: usize) {
    if aligners.len() < 2 {
        return;
    }
    let informative = |r: &Scored| {
        let class = r
            .pair
            .group
            .split(':')
            .next()
            .and_then(|c| c.parse::<u32>().ok());
        matches!(class, Some(3 | 5 | 6 | 7)) || r.pair.group.starts_with("duplex")
    };
    let mut rows: Vec<(&Scored, f32)> = scored
        .iter()
        .filter(|r| informative(r) && r.pair.label != Label::Unlabeled)
        .map(|r| {
            let max = r.scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let min = r.scores.iter().copied().fold(f32::INFINITY, f32::min);
            (r, max - min)
        })
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("== largest disagreements (classes 3, 5–7, duplex)");
    for (row, spread) in rows.into_iter().take(limit) {
        let scores: Vec<String> = aligners
            .iter()
            .zip(&row.scores)
            .map(|(kind, s)| format!("{kind} {s:.3}"))
            .collect();
        println!(
            "spread {spread:.3}  {} / {}  [{}]",
            row.pair.source,
            row.pair.group,
            scores.join(", ")
        );
        println!("    {}", row.pair.a);
        println!("    {}", row.pair.b);
    }
    println!();
}
