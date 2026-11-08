//! Packs word-pair counts (text or packed) for the search: keeps the pairs
//! whose words can be in the lexicon (CMUdict's, from the frequency list,
//! in the ENABLE list unless `--all-words`), optionally only the `--keep N`
//! most informative, and writes the packed format (see
//! `mishear::bigrams::BigramTable::write_packed`), a tenth the size of the
//! text.
//!
//! ```sh
//! cargo run --release -p mishear --example pack_bigrams -- IN.tsv data/bigrams-en.bin [--keep N]
//! ```
//!
//! A pair's information is `c·ln(1 + c / (μ·P(w)))` (μ = `--score-mu`,
//! default 100000): how much its count raises the likelihood of what was
//! counted over backing off to `P(w)`.

use std::collections::HashSet;
use std::path::Path;

use mime_reader::CmuDictionary;
use mishear::Lexicon;
use mishear::bigrams::BigramTable;
use mishear::lexicon::{CommonWords, read_frequencies};

fn main() -> Result<(), String> {
    let mut positional = Vec::new();
    let (mut keep, mut score_mu, mut all_words) = (None, 100_000.0f64, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--keep" => keep = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--keep N")?),
            "--score-mu" => {
                score_mu = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--score-mu F")?
            }
            "--all-words" => all_words = true,
            _ => positional.push(arg),
        }
    }
    let [input, output] = positional.as_slice() else {
        return Err("usage: pack_bigrams IN OUT [--keep N] [--score-mu F] [--all-words]".into());
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dict = CmuDictionary::load(root.join("../../data/CMUdict/cmudict-0.7b"))
        .map_err(|e| e.to_string())?;
    let frequencies = read_frequencies(&root.join("data/wordfreq-en.tsv"))?;
    let common = CommonWords::read(&root.join("../../data/ENABLE/enable1.txt"))?;
    let lexicon = Lexicon::build(&dict, &frequencies, usize::MAX, true, |w| {
        all_words || common.contains(w)
    });
    let words: HashSet<&str> = (0..lexicon.len() as u32).map(|w| lexicon.word(w)).collect();
    // `−ln P(w)`, for the information score.
    let surprisal = |w: &str| {
        lexicon
            .id(w)
            .map_or(f64::INFINITY, |id| f64::from(lexicon.surprisal(id)))
    };

    let table = BigramTable::read(Path::new(input))?;
    let packed = table.prune(
        |w| words.contains(w),
        keep,
        |w, c| {
            let c = f64::from(c);
            c * (1.0 + c / (score_mu * (-surprisal(w)).exp())).ln()
        },
    );
    let mut out =
        std::io::BufWriter::new(std::fs::File::create(output).map_err(|e| e.to_string())?);
    packed.write_packed(&mut out).map_err(|e| e.to_string())?;
    drop(out);
    let size = std::fs::metadata(output).map_err(|e| e.to_string())?.len();
    println!(
        "{} pairs in, {} kept over {} words -> {output} ({:.1} MB)",
        table.len(),
        packed.len(),
        packed.words().len(),
        size as f64 / 1e6
    );
    Ok(())
}
