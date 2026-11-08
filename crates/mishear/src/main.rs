//! `mishear` — fuzzy phonetic search, deformations and mishearings.
//!
//! `cargo run --release -p mishear -- help`

use std::path::{Path, PathBuf};
use std::time::Duration;

use mishear::accents;
use mishear::knobs;
use mishear::lexicon::CommonWords;
use mishear::phone::spell;
use mishear::{Deformation, Engine, MishearConfig, Ph, SearchConfig, search};

const USAGE: &str = "\
mishear — fuzzy phonetic search, accents and mishearings, on mime-reader

USAGE: cargo run --release -p mishear -- COMMAND [OPTIONS]

  search ARPABET…    the word sequences that best match these phonemes
  hear TEXT          other ways to hear TEXT's own sounds (mondegreens)
  deform TEXT        TEXT's phonemes before and after a deformation
  mishear TEXT       deform TEXT, then hear it back
  examples           mishear lines drawn from data/echo
  sweep TEXT         mishear TEXT at every combination of --lm-values,
                     --penalty-values and --pron-values (top -n each)
  cost               time the search on corpus lines
  list               accents and knobs

SEARCH OPTIONS
  -n N               how many results (default 8)
  --within X         every match costing at most X (otherwise the exact top N)
  --novelty F        mishear/hear/examples: cost per word shared with the line
                     (default 1.0; function words a fifth of that)
  --lm F             cost per nat of word surprisal, −ln P(word | previous):
                     how much common words are preferred (default 0.05; 0
                     ignores frequency)
  --word-penalty F   cost per word: higher prefers fewer, longer words
                     (default 0.1)
  --pronunciations first|all
                     match words under every CMUdict pronunciation or only
                     the first (default all)
  --bigram-prior F   bigram smoothing μ: how many words of evidence the
                     unigram frequencies are worth (default 100; larger
                     trusts word pairs less)
  --function-share F the share of --novelty a shared function word costs
                     (default 0.2)
  --vocab N          lexicon size: most frequent words (default 50000)
  --unigram          score words alone, not by the previous word too (the
                     bigram model, from data/bigrams-en.tsv, makes output
                     more fluent, at the cost of exactness: see search.rs)
  --all-words        keep names and brands (by default the lexicon keeps only
                     words /usr/share/dict/words has in lowercase, or whose
                     base form it has)

DEFORMATION OPTIONS (combine freely; applied in order)
  --accent NAME      a preset (see `list`)
  --knob NAME        a consonant knob, e.g. th-stopping (see `list`)
  --vowels SPEC      a vowel-space move: pull:X,Y,AMOUNT  shift:DX,DY
                     onset:DX,DY (diphthong starts only)  rotate:DEGREES
                     scale:F  mirror:front-back  mirror:height
                     (x: front 0 → back 1; y: low 0 → high 1), optionally
                     masked: …@tense @lax @front @back @diphthongs
                     Labov's chain-shift tendencies: tense-up:A lax-down:A
                     onsets-down:A back-to-front:A
  --intensity F      scale every step's rate (0–1; default 1)
  --seed N           which sites a partial rate picks (default 0)

SWEEP OPTIONS
  --lm-values A,B,…        (default: --lm)
  --penalty-values A,B,…   (default: --word-penalty)
  --pron-values first,all  (default: --pronunciations)

EXAMPLES OPTIONS
  --lines K          how many corpus lines (default 6)
  --public           only the public-domain poems (data/echo/public)
  --line-seed N      which lines (default 1)";

struct Options {
    command: String,
    positional: Vec<String>,
    results: usize,
    within: Option<f32>,
    novelty: f32,
    lm: f32,
    vocab: usize,
    deformation: Deformation,
    lines: usize,
    line_seed: u64,
    all_words: bool,
    unigram: bool,
    public_only: bool,
    word_penalty: f32,
    all_pronunciations: bool,
    bigram_prior: f32,
    function_share: f32,
    lm_values: Option<Vec<f32>>,
    penalty_values: Option<Vec<f32>>,
    pron_values: Option<Vec<bool>>,
}

fn parse(args: Vec<String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_else(|| "help".into());
    let mut o = Options {
        command,
        positional: Vec::new(),
        results: 8,
        within: None,
        novelty: 1.0,
        lm: 0.05,
        vocab: 50_000,
        deformation: Deformation::new(""),
        lines: 6,
        line_seed: 1,
        all_words: false,
        unigram: false,
        public_only: false,
        word_penalty: 0.1,
        all_pronunciations: true,
        bigram_prior: mishear::lexicon::BIGRAM_PRIOR,
        function_share: 0.2,
        lm_values: None,
        penalty_values: None,
        pron_values: None,
    };
    let mut intensity = 1.0;
    let mut seed = 0;
    let mut names: Vec<String> = Vec::new();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        let number = |s: String| s.parse::<f32>().map_err(|e| format!("{s}: {e}"));
        match arg.as_str() {
            "-n" => o.results = value("-n")?.parse().map_err(|e| format!("-n: {e}"))?,
            "--within" => o.within = Some(number(value("--within")?)?),
            "--novelty" => o.novelty = number(value("--novelty")?)?,
            "--lm" => o.lm = number(value("--lm")?)?,
            "--word-penalty" => o.word_penalty = number(value("--word-penalty")?)?,
            "--bigram-prior" => o.bigram_prior = number(value("--bigram-prior")?)?,
            "--function-share" => o.function_share = number(value("--function-share")?)?,
            "--pronunciations" => {
                o.all_pronunciations = pronunciations(&value("--pronunciations")?)?
            }
            "--lm-values" => o.lm_values = Some(numbers(&value("--lm-values")?)?),
            "--penalty-values" => o.penalty_values = Some(numbers(&value("--penalty-values")?)?),
            "--pron-values" => {
                o.pron_values = Some(
                    value("--pron-values")?
                        .split(',')
                        .map(pronunciations)
                        .collect::<Result<_, _>>()?,
                )
            }
            "--vocab" => {
                o.vocab = value("--vocab")?
                    .parse()
                    .map_err(|e| format!("--vocab: {e}"))?
            }
            "--lines" => {
                o.lines = value("--lines")?
                    .parse()
                    .map_err(|e| format!("--lines: {e}"))?
            }
            "--public" => o.public_only = true,
            "--line-seed" => {
                o.line_seed = value("--line-seed")?
                    .parse()
                    .map_err(|e| format!("--line-seed: {e}"))?
            }
            "--accent" => {
                let name = value("--accent")?;
                let accent = accents::preset(&name)
                    .ok_or_else(|| format!("unknown accent {name:?} (see `list`)"))?;
                o.deformation = std::mem::take(&mut o.deformation).then(accent);
                names.push(name);
            }
            "--knob" => {
                let name = value("--knob")?;
                let knob = knobs::consonant(&name)
                    .ok_or_else(|| format!("unknown knob {name:?} (see `list`)"))?;
                o.deformation = std::mem::take(&mut o.deformation).then(knob);
                names.push(name);
            }
            "--vowels" => {
                let spec = value("--vowels")?;
                let (moves, only) = knobs::vowel_knob(&spec)?;
                let step = Deformation::new(spec.clone()).vowels_only(moves, only);
                o.deformation = std::mem::take(&mut o.deformation).then(step);
                names.push(spec);
            }
            "--intensity" => intensity = number(value("--intensity")?)?,
            "--all-words" => o.all_words = true,
            "--unigram" => o.unigram = true,
            "--seed" => {
                seed = value("--seed")?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?
            }
            flag if flag.starts_with("--") || flag == "-h" => {
                return Err(format!("unknown option {flag}"));
            }
            _ => o.positional.push(arg),
        }
    }
    o.deformation = std::mem::take(&mut o.deformation).intensity(intensity);
    o.deformation.seed = seed;
    o.deformation.name = names.join(" + ");
    if intensity < 1.0 {
        o.deformation.name += &format!(" @ {intensity}");
    }
    Ok(o)
}

fn pronunciations(s: &str) -> Result<bool, String> {
    match s.trim() {
        "all" => Ok(true),
        "first" => Ok(false),
        other => Err(format!("pronunciations {other:?}: expected first or all")),
    }
}

fn numbers(s: &str) -> Result<Vec<f32>, String> {
    s.split(',')
        .map(|x| x.trim().parse::<f32>().map_err(|e| format!("{x}: {e}")))
        .collect()
}

fn crate_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The system word list that keeps names and brands out of the lexicon.
const COMMON_WORDS: &str = "/usr/share/dict/words";

fn engine(o: &Options) -> Result<Engine, String> {
    engine_with(o, o.vocab, o.all_pronunciations)
}

fn engine_with(o: &Options, vocab: usize, all_pronunciations: bool) -> Result<Engine, String> {
    let frequencies = crate_path("data/wordfreq-en.tsv");
    if !frequencies.exists() {
        return Err(format!(
            "{} is missing: generate it with scripts/wordfreq_list.py",
            frequencies.display()
        ));
    }
    let common = if o.all_words || !Path::new(COMMON_WORDS).exists() {
        None
    } else {
        Some(CommonWords::read(Path::new(COMMON_WORDS))?)
    };
    let mut engine = Engine::load(
        &crate_path("../../data/CMUdict/cmudict-0.7b"),
        &frequencies,
        vocab,
        all_pronunciations,
        common.as_ref(),
    )?;
    let bigrams = crate_path("data/bigrams-en.tsv");
    if bigrams.exists() {
        engine.lexicon.load_bigrams(&bigrams, o.bigram_prior)?;
    }
    Ok(engine)
}

fn search_config(o: &Options) -> SearchConfig {
    SearchConfig {
        results: o.results,
        bound: o.within.unwrap_or(2.0),
        lm_weight: o.lm,
        word_penalty: o.word_penalty,
        bigrams: !o.unigram,
        ..SearchConfig::default()
    }
}

fn mishear_config(o: &Options) -> MishearConfig {
    MishearConfig {
        search: search_config(o),
        novelty: o.novelty,
        function_word_share: o.function_share,
        // `--within X` fixes the bound; otherwise it grows to the top N.
        max_bound: o.within.unwrap_or(30.0),
    }
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}

fn print_mishearing(engine: &Engine, text: &str, o: &Options) -> Result<(), String> {
    let m = engine.mishear(text, &o.deformation, &mishear_config(o))?;
    println!("{text}");
    let before: Vec<String> = m.reading.phonemes.iter().map(|w| spell(w)).collect();
    let after: Vec<String> = m.deformed.iter().map(|w| spell(w)).collect();
    if before != after {
        println!("  sounds   {}", before.join(" · "));
        println!("  deformed {}", after.join(" · "));
    }
    for h in &m.heard {
        println!(
            "  {:>5.2}  phonetic {:>4.2}  shared {}  {}",
            h.cost, h.phonetic, h.shared, h.text
        );
    }
    if m.heard.is_empty() {
        println!("  (nothing within the bound)");
    }
    println!(
        "  [{} · bound {:.1} after {} rounds · {}]",
        if o.deformation.steps.is_empty() {
            "no deformation"
        } else {
            o.deformation.name.as_str()
        },
        m.stats.bound,
        m.stats.rounds,
        ms(m.stats.elapsed)
    );
    Ok(())
}

/// Every line of the poems under `data/echo` (public, and private when
/// present), as `(poem, line)`.
fn corpus_lines(public_only: bool) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    let dirs: &[&str] = if public_only {
        &["public"]
    } else {
        &["public", "private"]
    };
    for dir in dirs {
        let root = crate_path(&format!("../../data/echo/{dir}"));
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "txt") {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    let body = text.split_once("\n\n").map_or("", |(_, b)| b);
                    for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
                        lines.push((name.clone(), line.to_string()));
                    }
                }
            }
        }
    }
    lines
}

/// `count` lines of 4–10 words that the engine can read, picked by `seed`.
fn sample_lines(
    engine: &Engine,
    count: usize,
    seed: u64,
    public_only: bool,
) -> Vec<(String, String)> {
    let mut all = corpus_lines(public_only);
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    // Fisher–Yates with xorshift.
    for i in (1..all.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        all.swap(i, (state % (i as u64 + 1)) as usize);
    }
    all.into_iter()
        .filter(|(_, line)| {
            let words = line.split_whitespace().count();
            (4..=10).contains(&words) && engine.read(line).is_ok()
        })
        .take(count)
        .collect()
}

fn run(o: Options) -> Result<(), String> {
    match o.command.as_str() {
        "help" | "-h" | "--help" => println!("{USAGE}"),
        "list" => {
            println!("accents:");
            for (name, about) in accents::PRESETS {
                println!("  {name:<14} {about}");
            }
            println!("consonant knobs: {}", knobs::CONSONANT_KNOBS.join(", "));
            println!(
                "vowel knobs: pull:X,Y,AMOUNT shift:DX,DY onset:DX,DY rotate:DEG scale:F mirror:front-back mirror:height"
            );
            println!(
                "  masks: @tense @lax @front @back @diphthongs; Labov: tense-up:A lax-down:A onsets-down:A back-to-front:A"
            );
        }
        "search" => {
            let input =
                Ph::parse_all(&o.positional.join(" ")).ok_or("expected ARPAbet phonemes")?;
            let engine = engine(&o)?;
            let config = search_config(&o);
            let (matches, stats) = match o.within {
                Some(_) => search::search(&input, &engine.lexicon, &engine.cost, &config),
                None => engine.hear(&input, &config, 30.0),
            };
            println!("{}", spell(&input));
            for m in &matches {
                println!(
                    "  {:>5.2}  phonetic {:>4.2}  {}",
                    m.cost,
                    m.phonetic,
                    m.text(&engine.lexicon)
                );
            }
            println!(
                "  [{} matches · bound {:.1} after {} rounds · {} trie nodes · {}]",
                matches.len(),
                stats.bound,
                stats.rounds,
                stats.nodes,
                ms(stats.elapsed)
            );
        }
        "hear" | "mishear" => {
            let engine = engine(&o)?;
            let text = o.positional.join(" ");
            let o = if o.command == "hear" {
                Options {
                    deformation: Deformation::new(""),
                    ..o
                }
            } else {
                o
            };
            print_mishearing(&engine, &text, &o)?;
        }
        "deform" => {
            let engine = engine(&o)?;
            let text = o.positional.join(" ");
            let reading = engine.read(&text)?;
            let after = o
                .deformation
                .apply_spelled(&reading.phonemes, &reading.words);
            for ((word, before), after) in reading.words.iter().zip(&reading.phonemes).zip(&after) {
                let mark = if before == after { " " } else { "*" };
                println!("{mark} {word:<14} {:<24} {}", spell(before), spell(after));
            }
        }
        "examples" => {
            let engine = engine(&o)?;
            for (poem, line) in sample_lines(&engine, o.lines, o.line_seed, o.public_only) {
                println!("— {poem}");
                print_mishearing(&engine, &line, &o)?;
                println!();
            }
        }
        "cost" => cost_report(&o)?,
        "sweep" => sweep(&o)?,
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
    Ok(())
}

/// Mishears one line at every combination of the prior's settings.
fn sweep(o: &Options) -> Result<(), String> {
    let text = o.positional.join(" ");
    let lms = o.lm_values.clone().unwrap_or_else(|| vec![o.lm]);
    let penalties = o
        .penalty_values
        .clone()
        .unwrap_or_else(|| vec![o.word_penalty]);
    let prons = o
        .pron_values
        .clone()
        .unwrap_or_else(|| vec![o.all_pronunciations]);
    println!("{text}");
    if !o.deformation.steps.is_empty() {
        println!("  [{}]", o.deformation.name);
    }
    for &all in &prons {
        let engine = engine_with(o, o.vocab, all)?;
        for &lm in &lms {
            for &penalty in &penalties {
                let options = Options {
                    command: String::new(),
                    positional: Vec::new(),
                    deformation: o.deformation.clone(),
                    lm_values: None,
                    penalty_values: None,
                    pron_values: None,
                    lm,
                    word_penalty: penalty,
                    all_pronunciations: all,
                    ..*o
                };
                let m = engine.mishear(&text, &options.deformation, &mishear_config(&options))?;
                let heard: Vec<String> = m
                    .heard
                    .iter()
                    .map(|h| format!("{} ({:.2})", h.text, h.phonetic))
                    .collect();
                println!(
                    "  lm {lm:<5} penalty {penalty:<4} {:<5} │ {}  [{}]",
                    if all { "all" } else { "first" },
                    heard.join(" · "),
                    ms(m.stats.elapsed)
                );
            }
        }
    }
    Ok(())
}

/// Times the exact top-N search and the bounded search on corpus lines.
fn cost_report(o: &Options) -> Result<(), String> {
    for vocab in [20_000, 50_000, 100_000] {
        let engine = engine_with(o, vocab, o.all_pronunciations)?;
        let lines = sample_lines(&engine, o.lines.max(20), o.line_seed, o.public_only);
        println!(
            "== vocabulary {} words ({} trie nodes), {} lines",
            engine.lexicon.len(),
            engine.lexicon.node_count(),
            lines.len()
        );
        let inputs: Vec<Vec<Ph>> = lines
            .iter()
            .map(|(_, l)| engine.read(l).unwrap().phonemes.concat())
            .collect();
        let lengths: Vec<usize> = inputs.iter().map(Vec::len).collect();
        println!(
            "   phonemes per line: {}–{} (mean {:.0})",
            lengths.iter().min().unwrap(),
            lengths.iter().max().unwrap(),
            lengths.iter().sum::<usize>() as f64 / lengths.len() as f64
        );
        let summarize = |label: String, times: &mut Vec<Duration>, extra: String| {
            times.sort();
            println!(
                "   {label:<28} median {:>9}  max {:>9}  {extra}",
                ms(times[times.len() / 2]),
                ms(*times.last().unwrap())
            );
        };
        for results in [1, 10] {
            let config = SearchConfig {
                results,
                bound: 1.0,
                ..SearchConfig::default()
            };
            let mut times = Vec::new();
            let mut bounds = Vec::new();
            for input in &inputs {
                let (_, stats) = engine.hear(input, &config, 60.0);
                times.push(stats.elapsed);
                bounds.push(stats.bound);
            }
            bounds.sort_by(f32::total_cmp);
            summarize(
                format!("exact top {results}"),
                &mut times,
                format!("final bound median {:.1}", bounds[bounds.len() / 2]),
            );
        }
        for within in [2.0, 4.0, 6.0, 8.0] {
            let config = SearchConfig {
                results: 10,
                bound: within,
                ..SearchConfig::default()
            };
            let mut times = Vec::new();
            let mut found = 0;
            for input in &inputs {
                let (matches, stats) =
                    search::search(input, &engine.lexicon, &engine.cost, &config);
                times.push(stats.elapsed);
                found += usize::from(!matches.is_empty());
            }
            summarize(
                format!("10 within {within}"),
                &mut times,
                format!("lines with a match {found}/{}", inputs.len()),
            );
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = parse(args).and_then(run) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
