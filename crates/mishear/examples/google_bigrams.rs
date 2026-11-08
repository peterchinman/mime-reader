//! Sums Google Books Ngram (2020, "v3") English 2-gram counts into word-pair
//! counts for the bigram model: pairs of words in data/wordfreq-en.tsv,
//! lowercased, leaving out the part-of-speech-tagged variants ("gazed_VERB"),
//! summed over every year and over the years from `--since` on. The Google
//! data is CC BY 3.0.
//!
//! The 2-grams come in files split by first word, so all of them are
//! needed; stream them through, one after another:
//!
//! ```sh
//! for u in $(cat urls); do curl -sS --fail "$u" | gunzip -c; done |
//!   cargo run --release -p mishear --example google_bigrams -- --since 1990 --out /tmp/google
//! ```
//!
//! where `urls` lists a corpus's 2-gram files, from its index page (e.g.
//! http://storage.googleapis.com/books/ngrams/books/20200217/eng-fiction/eng-fiction-2-ngrams_exports.html).
//! Writes OUT-all.tsv and OUT-since.tsv in scripts/bigrams.py's format:
//! `left<TAB>right<TAB>count` for every pair, and `left<TAB>*<TAB>total`, how
//! often the left word is followed by any word (punctuation aside). Google
//! normalizes negations ("don't" becomes "do not") and splits other
//! contractions ("they 're"), so contractions get no pairs.

use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};
use std::time::Instant;

fn main() {
    let mut since = 1990u32;
    let mut out = String::from("google");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--since" => {
                since = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--since YEAR")
            }
            "--out" => out = args.next().expect("--out PREFIX"),
            other => panic!("unknown argument {other}"),
        }
    }

    let vocab_path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/wordfreq-en.tsv");
    let mut words: Vec<String> = Vec::new();
    let mut ids: HashMap<String, u32> = HashMap::new();
    for line in std::fs::read_to_string(vocab_path)
        .expect(vocab_path)
        .lines()
    {
        let word = line.split('\t').next().unwrap_or_default();
        if !ids.contains_key(word) {
            ids.insert(word.to_string(), words.len() as u32);
            words.push(word.to_string());
        }
    }

    // [all years, since]
    let mut totals = vec![[0u64; 2]; words.len()];
    let mut pairs: HashMap<u64, [u64; 2]> = HashMap::new();
    let (mut lines, mut kept) = (0u64, 0u64);
    let start = Instant::now();
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    let mut lower_a = String::new();
    let mut lower_b = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            // A bad byte in one line shouldn't end the run.
            Err(_) => continue,
        }
        lines += 1;
        if lines % 20_000_000 == 0 {
            eprintln!(
                "{:>6.0}s {lines} lines, {kept} kept, {} pairs",
                start.elapsed().as_secs_f64(),
                pairs.len()
            );
        }
        let mut fields = line.trim_end().split('\t');
        let Some((a, b)) = fields.next().and_then(|g| g.split_once(' ')) else {
            continue;
        };
        if a.contains('_') || b.contains('_') || b.contains(' ') {
            continue;
        }
        lower_a.clear();
        lower_a.extend(a.chars().flat_map(char::to_lowercase));
        let Some(&v) = ids.get(lower_a.as_str()) else {
            continue;
        };
        if !b.chars().any(char::is_alphabetic) {
            continue; // punctuation ends the run, as in the subtitles
        }
        let mut sum = [0u64; 2];
        for year in fields {
            let mut parts = year.split(',');
            let (Some(y), Some(m)) = (parts.next(), parts.next()) else {
                continue;
            };
            let (Ok(y), Ok(m)) = (y.parse::<u32>(), m.parse::<u64>()) else {
                continue;
            };
            sum[0] += m;
            if y >= since {
                sum[1] += m;
            }
        }
        totals[v as usize][0] += sum[0];
        totals[v as usize][1] += sum[1];
        lower_b.clear();
        lower_b.extend(b.chars().flat_map(char::to_lowercase));
        if let Some(&w) = ids.get(lower_b.as_str()) {
            let entry = pairs
                .entry((u64::from(v) << 32) | u64::from(w))
                .or_default();
            entry[0] += sum[0];
            entry[1] += sum[1];
            kept += 1;
        }
    }
    eprintln!(
        "{:.0}s: {lines} lines, {kept} kept, {} pairs",
        start.elapsed().as_secs_f64(),
        pairs.len()
    );

    let mut keys: Vec<u64> = pairs.keys().copied().collect();
    keys.sort_unstable();
    for (k, name) in [(0, "all"), (1, "since")] {
        let path = format!("{out}-{name}.tsv");
        let mut f = BufWriter::new(std::fs::File::create(&path).expect(&path));
        for (v, total) in totals.iter().enumerate() {
            if total[k] > 0 {
                writeln!(f, "{}\t*\t{}", words[v], total[k]).unwrap();
            }
        }
        for &key in &keys {
            let count = pairs[&key][k];
            if count > 0 {
                let (v, w) = ((key >> 32) as usize, (key & 0xFFFF_FFFF) as usize);
                writeln!(f, "{}\t{}\t{count}", words[v], words[w]).unwrap();
            }
        }
        eprintln!("wrote {path}");
    }
}
