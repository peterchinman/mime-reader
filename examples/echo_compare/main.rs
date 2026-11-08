//! Compares the echo aligners on labeled data: synthetic fixture pairs,
//! real repeating-form poems (template echoes vs every other line pair),
//! and mutations of corpus lines. Reports per-group similarity, separation
//! (AUC, best threshold, margin), pass rates at the shipped thresholds,
//! per-poem ranking and threshold transfer. Each aligner scores under its
//! own tuned config (`EchoConfig::for_aligner`), plus any override.
//!
//! `cargo run --release --example echo_compare -- --help`

#[cfg(not(target_arch = "wasm32"))]
mod args;
#[cfg(not(target_arch = "wasm32"))]
mod corpus;
#[cfg(not(target_arch = "wasm32"))]
mod dump;
#[cfg(not(target_arch = "wasm32"))]
mod metrics;
#[cfg(not(target_arch = "wasm32"))]
mod mutate;
#[cfg(not(target_arch = "wasm32"))]
mod report;
#[cfg(not(target_arch = "wasm32"))]
mod search;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Scored;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::{BTreeMap, HashMap};
    use std::time::Instant;

    use mime_reader::echo::EchoDetector;
    use mime_reader::line::Line;
    use mime_reader::{CmuDictionary, EchoError, PronouncingDictionary, PronunciationMode};

    use crate::args::{Args, USAGE, effective_config, repo_path};
    use crate::corpus::{self, LabeledPair};
    use crate::{dump, mutate, report};

    /// Mutated pairs generated per mutation kind.
    const MUTATIONS_PER_KIND: usize = 40;

    /// A labeled pair with one similarity per aligner (in `--aligner` order).
    pub struct Scored {
        pub pair: LabeledPair,
        pub scores: Vec<f32>,
    }

    pub fn run() -> Result<(), String> {
        let args = Args::parse(std::env::args().skip(1))?;
        if args.help {
            println!("{USAGE}");
            return Ok(());
        }
        let env = std::env::var("ECHO_CONFIG").ok();
        let mut detectors = Vec::with_capacity(args.aligners.len());
        for &kind in &args.aligners {
            let config = effective_config(&args, env.as_deref(), kind)?;
            let detector = EchoDetector::with_config(config);
            report::config(kind, &config, &detector);
            detectors.push(detector);
        }
        let overridden = args.config.is_some() || env.is_some();

        let dict = CmuDictionary::load(repo_path("data/CMUdict/cmudict-0.7b"))
            .map_err(|e| format!("dictionary: {e}"))?
            .with_pronunciation_mode(PronunciationMode::AllowDerived);

        // Inputs.
        let mut pairs: Vec<LabeledPair> = Vec::new();
        if let Some(path) = &args.fixture {
            pairs.extend(corpus::load_fixture(path)?);
        }
        let mut poems = Vec::new();
        for dir in &args.corpora {
            // Anything under a `private` directory is private.
            let source = if dir.components().any(|c| c.as_os_str() == "private") {
                "private"
            } else {
                "public"
            };
            if !dir.is_dir() {
                return Err(format!("corpus {} is not a directory", dir.display()));
            }
            poems.extend(corpus::load_dir(dir, source)?);
        }
        let skipped: Vec<String> = poems
            .iter()
            .filter(|p| args.max_lines.is_some_and(|max| p.lines.len() > max))
            .map(|p| p.id())
            .collect();
        let excluded: Vec<String> = poems
            .iter()
            .filter_map(|p| p.excluded.as_ref().map(|why| format!("{} ({why})", p.id())))
            .collect();
        poems.retain(|p| p.excluded.is_none());
        poems.retain(|p| args.max_lines.is_none_or(|max| p.lines.len() <= max));
        if let Some(forms) = &args.forms {
            poems.retain(|p| forms.contains(&p.form));
        }
        for (index, poem) in poems.iter().enumerate() {
            pairs.extend(corpus::poem_pairs(poem, index));
        }
        if args.mutations && args.search.is_none() {
            let lines: Vec<String> = poems.iter().flat_map(|p| p.lines.iter().cloned()).collect();
            pairs.extend(mutate::mutation_pairs(
                &lines,
                MUTATIONS_PER_KIND,
                args.seed,
                &dict,
            ));
        }
        let mut by_source: BTreeMap<&str, usize> = BTreeMap::new();
        for pair in &pairs {
            *by_source.entry(pair.source.as_str()).or_default() += 1;
        }
        println!("== inputs");
        let private = poems.iter().filter(|p| p.source == "private").count();
        println!(
            "poems: {} ({} public, {private} private){}",
            poems.len(),
            poems.len() - private,
            if skipped.is_empty() {
                String::new()
            } else {
                format!("; skipped over --max-lines: {}", skipped.join(", "))
            }
        );
        if !excluded.is_empty() {
            println!("excluded: {}", excluded.join("; "));
        }
        let counts: Vec<String> = by_source.iter().map(|(s, n)| format!("{s} {n}")).collect();
        println!("pairs: {}", counts.join(", "));

        // Read every distinct line once; drop pairs with unknown words.
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut lines: Vec<Line> = Vec::new();
        let mut line_of = |text: &str| -> usize {
            *index.entry(text.to_string()).or_insert_with(|| {
                lines.push(Line::new(text, &dict));
                lines.len() - 1
            })
        };
        let pair_lines: Vec<(usize, usize)> = pairs
            .iter()
            .map(|p| (line_of(&p.a), line_of(&p.b)))
            .collect();
        let mut oov_words: HashMap<String, usize> = HashMap::new();
        let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
        let mut kept: Vec<(LabeledPair, usize, usize)> = Vec::new();
        for (pair, (a, b)) in pairs.into_iter().zip(pair_lines) {
            let unknown: Vec<&str> = lines[a]
                .unknown_words()
                .into_iter()
                .chain(lines[b].unknown_words())
                .collect();
            if unknown.is_empty() && !lines[a].words.is_empty() && !lines[b].words.is_empty() {
                kept.push((pair, a, b));
            } else {
                for word in unknown {
                    *oov_words.entry(word.to_lowercase()).or_default() += 1;
                }
                *dropped.entry(pair.source.clone()).or_default() += 1;
            }
        }
        if !dropped.is_empty() {
            let counts: Vec<String> = dropped.iter().map(|(s, n)| format!("{s} {n}")).collect();
            let mut top: Vec<(String, usize)> = oov_words.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let top: Vec<String> = top
                .iter()
                .take(15)
                .map(|(w, n)| format!("{w} ×{n}"))
                .collect();
            println!("dropped for unknown words: {}", counts.join(", "));
            println!("top unknown words: {}", top.join(", "));
        }
        println!("scored pairs: {}", kept.len());
        println!();

        if let Some(trials) = args.search {
            return crate::search::run(&args, trials, &poems, &kept, &lines);
        }

        // Score.
        println!("== timing");
        let mut scores = vec![Vec::with_capacity(kept.len()); args.aligners.len()];
        for (k, &kind) in args.aligners.iter().enumerate() {
            let start = Instant::now();
            let detector = &detectors[k];
            for (pair, a, b) in &kept {
                let similarity = match detector.compare_lines_with(kind, &lines[*a], &lines[*b]) {
                    Ok(echo) => echo.alignment.similarity,
                    Err(EchoError::UnknownWord(_) | EchoError::EmptyLine) => {
                        unreachable!("filtered above: {} / {}", pair.a, pair.b)
                    }
                };
                scores[k].push(similarity);
            }
            let elapsed = start.elapsed();
            println!(
                "{:<16} {:>8.1} ms  ({:.1} µs/pair)",
                kind.to_string(),
                elapsed.as_secs_f64() * 1e3,
                elapsed.as_secs_f64() * 1e6 / kept.len().max(1) as f64
            );
        }
        println!();
        let scored: Vec<Scored> = kept
            .into_iter()
            .enumerate()
            .map(|(n, (pair, _, _))| Scored {
                pair,
                scores: scores.iter().map(|column| column[n]).collect(),
            })
            .collect();

        // Report.
        report::groups(&scored, &args.aligners);
        let sets = report::eval_sets(&scored);
        let best = report::separation(&sets, &args.aligners);
        report::shipped_thresholds(&scored, &poems, &args.aligners, overridden);
        report::transfer(&sets, &best, &args.aligners);
        report::poems(&scored, &poems, &args.aligners, args.by_poem);
        report::disagreements(&scored, &args.aligners, 12);

        if let Some(path) = &args.dump {
            let ids: Vec<String> = poems.iter().map(|p| p.id()).collect();
            dump::write(path, &scored, &args.aligners, &ids)?;
            println!("wrote {} rows to {}", scored.len(), path.display());
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(error) = native::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
