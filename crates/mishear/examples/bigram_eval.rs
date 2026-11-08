//! Scores word-pair count files (scripts/bigrams.py's format) on test word
//! pairs (`w1<TAB>w2` lines): the mean `−ln P(w2 | w1)` in nats under the
//! model the search uses,
//! `P(w | v) = (count(v, w) + μ·P(w)) / (count(v) + μ)` with `P(w)` from
//! wordfreq, for each smoothing strength and pruning level. Lower is better;
//! the unigram model (`P(w)` alone) is the baseline. Also reports coverage:
//! the share of test pairs the counts have.
//!
//! `--keep N,M,…` also prunes each counts file to its N (M, …) most useful
//! pairs, two ways: by count, and by information, `c·ln(1 + c / (μ·P(w)))`
//! at `--score-mu` (how much the pair's count raises the likelihood of what
//! was counted over backing off to `P(w)`). `--write PATH` writes the last
//! file pruned by information to the last `--keep` size.
//!
//! ```sh
//! cargo run --release -p mishear --example bigram_eval -- \
//!   --test subs.tsv --test wiki.tsv --counts data/bigrams-en.tsv \
//!   --mu 300,1000,3000,10000 --min-count 1,3,100
//! ```

use rustc_hash::FxHashMap;

fn list<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split(',')
        .map(|x| x.trim().parse().ok().expect("a number"))
        .collect()
}

fn main() {
    let (mut tests, mut counts) = (Vec::<String>::new(), Vec::<String>::new());
    let mut mus: Vec<f64> = vec![300.0, 1000.0, 3000.0, 10000.0];
    let mut mins: Vec<f64> = vec![1.0];
    let mut keeps: Vec<usize> = Vec::new();
    let mut score_mu = 100_000.0f64;
    let mut write: Option<String> = None;
    // Give each left word's uncounted mass (pruned pairs, unlisted words)
    // to the backoff: P(w | v) = (c + (μ + r_v)·P(w)) / (N_v + μ).
    let mut redistribute = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value");
        match arg.as_str() {
            "--test" => tests.push(value()),
            "--counts" => counts.push(value()),
            "--mu" => mus = list(&value()),
            "--min-count" => mins = list(&value()),
            "--keep" => keeps = list(&value()),
            "--score-mu" => score_mu = value().parse().expect("a number"),
            "--write" => write = Some(value()),
            "--redistribute" => redistribute = true,
            other => panic!("unknown argument {other}"),
        }
    }

    let vocab_path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/wordfreq-en.tsv");
    let mut ids: FxHashMap<String, u32> = FxHashMap::default();
    let mut unigram: Vec<f64> = Vec::new();
    for line in std::fs::read_to_string(vocab_path)
        .expect(vocab_path)
        .lines()
    {
        let (word, zipf) = line.split_once('\t').expect("word<TAB>zipf");
        if !ids.contains_key(word) {
            ids.insert(word.to_string(), unigram.len() as u32);
            unigram.push(10f64.powf(zipf.parse::<f64>().expect("zipf") - 9.0));
        }
    }
    let key = |v: u32, w: u32| (u64::from(v) << 32) | u64::from(w);

    let test_sets: Vec<(String, Vec<(u32, u32)>)> = tests
        .iter()
        .map(|path| {
            let pairs = std::fs::read_to_string(path)
                .expect(path)
                .lines()
                .filter_map(|l| l.split_once('\t'))
                .filter_map(|(a, b)| Some((*ids.get(a)?, *ids.get(b)?)))
                .collect();
            (path.clone(), pairs)
        })
        .collect();
    for (name, pairs) in &test_sets {
        let uni = -pairs
            .iter()
            .map(|&(_, w)| unigram[w as usize].ln())
            .sum::<f64>()
            / pairs.len() as f64;
        println!("{name}: {} pairs, unigram {uni:.3} nats", pairs.len());
    }

    let mut words: Vec<String> = vec![String::new(); unigram.len()];
    for (w, &id) in &ids {
        words[id as usize] = w.clone();
    }
    let score =
        |test: &[(u32, u32)], pairs: &FxHashMap<u64, f64>, totals: &[f64], mu: f64| -> f64 {
            // The mass each left word's listed pairs don't account for.
            let mut rest: Vec<f64> = totals.to_vec();
            if redistribute {
                for (&k, &c) in pairs {
                    rest[(k >> 32) as usize] -= c;
                }
            }
            let s: f64 = test
                .iter()
                .map(|&(v, w)| {
                    let c = pairs.get(&key(v, w)).copied().unwrap_or(0.0);
                    let prior = if redistribute {
                        mu + rest[v as usize].max(0.0)
                    } else {
                        mu
                    };
                    -((c + prior * unigram[w as usize]) / (totals[v as usize] + mu)).ln()
                })
                .sum();
            s / test.len() as f64
        };
    for path in &counts {
        let mut pairs: FxHashMap<u64, f64> = FxHashMap::default();
        let mut totals = vec![0f64; unigram.len()];
        for line in std::fs::read_to_string(path).expect(path).lines() {
            let mut f = line.split('\t');
            let (Some(a), Some(b), Some(c)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let (Some(&v), Ok(c)) = (ids.get(a), c.parse::<f64>()) else {
                continue;
            };
            if b == "*" {
                totals[v as usize] = c;
            } else if let Some(&w) = ids.get(b) {
                pairs.insert(key(v, w), c);
            }
        }
        println!("\n{path}: {} pairs", pairs.len());
        if !keeps.is_empty() {
            let mut by_count: Vec<(u64, f64)> = pairs.iter().map(|(&k, &c)| (k, c)).collect();
            by_count.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
            let mut by_info: Vec<(u64, f64)> = pairs
                .iter()
                .map(|(&k, &c)| {
                    let p = unigram[(k & 0xFFFF_FFFF) as usize];
                    (k, c * (1.0 + c / (score_mu * p)).ln())
                })
                .collect();
            by_info.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
            for &n in &keeps {
                for (how, order) in [("count", &by_count), ("information", &by_info)] {
                    let kept: FxHashMap<u64, f64> =
                        order.iter().take(n).map(|&(k, _)| (k, pairs[&k])).collect();
                    for (name, test) in &test_sets {
                        let row: Vec<String> = mus
                            .iter()
                            .map(|&mu| format!("μ {mu}: {:.3}", score(test, &kept, &totals, mu)))
                            .collect();
                        println!(
                            "  top {n} by {how}, {}: {}",
                            name.rsplit('/').next().unwrap_or(name),
                            row.join(", ")
                        );
                    }
                }
            }
            if let (Some(out), Some(&n)) = (&write, keeps.last()) {
                use std::io::Write;
                let mut f = std::io::BufWriter::new(std::fs::File::create(out).expect(out));
                for (v, &t) in totals.iter().enumerate() {
                    if t > 0.0 {
                        writeln!(f, "{}\t*\t{t}", words[v]).unwrap();
                    }
                }
                let mut kept: Vec<u64> = by_info.iter().take(n).map(|&(k, _)| k).collect();
                kept.sort_unstable();
                for k in kept {
                    let (v, w) = ((k >> 32) as usize, (k & 0xFFFF_FFFF) as usize);
                    writeln!(f, "{}\t{}\t{}", words[v], words[w], pairs[&k]).unwrap();
                }
                println!("  wrote {out}: top {n} by information");
            }
            continue;
        }
        for &min in &mins {
            let kept = pairs.values().filter(|&&c| c >= min).count();
            for (name, test) in &test_sets {
                let covered = test
                    .iter()
                    .filter(|&&(v, w)| pairs.get(&key(v, w)).is_some_and(|&c| c >= min))
                    .count();
                let row: Vec<String> = mus
                    .iter()
                    .map(|&mu| {
                        let s: f64 = test
                            .iter()
                            .map(|&(v, w)| {
                                let c = pairs.get(&key(v, w)).copied().filter(|&c| c >= min);
                                let p = (c.unwrap_or(0.0) + mu * unigram[w as usize])
                                    / (totals[v as usize] + mu);
                                -p.ln()
                            })
                            .sum();
                        format!("μ {mu}: {:.3}", s / test.len() as f64)
                    })
                    .collect();
                println!(
                    "  pairs ≥ {min} ({kept} kept), {}: coverage {:.0}%, {}",
                    name.rsplit('/').next().unwrap_or(name),
                    100.0 * covered as f64 / test.len() as f64,
                    row.join(", ")
                );
            }
        }
    }
}
