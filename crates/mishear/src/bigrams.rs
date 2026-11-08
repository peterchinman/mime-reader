//! Word-pair counts for the bigram model, as loaded: indexed by their own
//! word list, so one table serves any lexicon (see
//! [`Lexicon::attach_bigrams`](crate::Lexicon::attach_bigrams)).
//!
//! Read from text, `left<TAB>right<TAB>count` lines with
//! `left<TAB>*<TAB>total` lines for how often each left word is followed by
//! any word; or from the packed format, a tenth the size (see
//! [`BigramTable::write_packed`]). Either way a count is kept as one byte,
//! `round(8·ln c)`: within 6%, which the model doesn't notice.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// The packed format's first bytes.
const MAGIC: &[u8; 8] = b"mshbg01\n";

pub struct BigramTable {
    /// The table's words; pairs refer to them by index.
    words: Vec<String>,
    index: HashMap<String, u32>,
    /// How often each word is followed by any word.
    totals: Vec<f64>,
    /// Word `v`'s pairs: `rights[offsets[v]..offsets[v + 1]]`, ascending,
    /// with their `counts`.
    offsets: Vec<u32>,
    rights: Vec<u32>,
    counts: Vec<u8>,
}

fn quantize(count: f64) -> u8 {
    (8.0 * count.max(1.0).ln()).round().min(255.0) as u8
}

/// The count a byte stands for.
pub fn dequantize(q: u8) -> f32 {
    static COUNTS: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    COUNTS.get_or_init(|| std::array::from_fn(|q| (q as f32 / 8.0).exp()))[q as usize]
}

impl BigramTable {
    /// Reads a counts file, text or packed.
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a counts file, text or packed.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.starts_with(MAGIC) {
            Self::parse_packed(&bytes[MAGIC.len()..])
        } else {
            Self::parse_text(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
        }
    }

    /// Parses the text format.
    pub fn parse_text(text: &str) -> Result<Self, String> {
        let mut words: Vec<String> = Vec::new();
        let mut index: HashMap<String, u32> = HashMap::new();
        let mut id = |word: &str, words: &mut Vec<String>| -> u32 {
            *index.entry(word.to_string()).or_insert_with(|| {
                words.push(word.to_string());
                (words.len() - 1) as u32
            })
        };
        let mut totals: Vec<(u32, f64)> = Vec::new();
        let mut pairs: Vec<(u32, u32, u8)> = Vec::new();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let mut fields = line.split('\t');
            let (Some(left), Some(right), Some(count)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(format!("malformed line {line:?}"));
            };
            let count: f64 = count.parse().map_err(|e| format!("{line:?}: {e}"))?;
            let v = id(left, &mut words);
            if right == "*" {
                totals.push((v, count));
            } else {
                let w = id(right, &mut words);
                pairs.push((v, w, quantize(count)));
            }
        }
        let mut table_totals = vec![0.0; words.len()];
        for (v, t) in totals {
            table_totals[v as usize] = t;
        }
        Ok(Self::from_pairs(words, table_totals, pairs))
    }

    /// A table of `words`, their `totals`, and `(left, right, byte)` pairs.
    fn from_pairs(words: Vec<String>, totals: Vec<f64>, mut pairs: Vec<(u32, u32, u8)>) -> Self {
        pairs.sort_unstable_by_key(|&(v, w, _)| (v, w));
        pairs.dedup_by_key(|&mut (v, w, _)| (v, w));
        let mut offsets = vec![0u32; words.len() + 1];
        for &(v, _, _) in &pairs {
            offsets[v as usize + 1] += 1;
        }
        for v in 0..words.len() {
            offsets[v + 1] += offsets[v];
        }
        let index = words
            .iter()
            .enumerate()
            .map(|(i, w)| (w.clone(), i as u32))
            .collect();
        Self {
            words,
            index,
            totals,
            offsets,
            rights: pairs.iter().map(|&(_, w, _)| w).collect(),
            counts: pairs.iter().map(|&(_, _, q)| q).collect(),
        }
    }

    /// The packed format, after [`MAGIC`]: five blocks, each a little-endian
    /// u32 byte length and its bytes:
    /// 1. the words, one per line;
    /// 2. each word's total, a varint;
    /// 3. each word's number of pairs, a varint;
    /// 4. each pair's right word, as the gap from the previous one in its
    ///    row (or from -1), a varint;
    /// 5. each pair's count, one byte (`round(8·ln c)`).
    pub fn write_packed(&self, out: &mut impl Write) -> std::io::Result<()> {
        let mut words = String::new();
        for w in &self.words {
            words.push_str(w);
            words.push('\n');
        }
        let (mut totals, mut lens, mut gaps) = (Vec::new(), Vec::new(), Vec::new());
        for &t in &self.totals {
            varint(t.round() as u64, &mut totals);
        }
        for v in 0..self.words.len() {
            let row = self.row(v as u32);
            varint(row.0.len() as u64, &mut lens);
            let mut previous: i64 = -1;
            for &w in row.0 {
                varint((i64::from(w) - previous - 1) as u64, &mut gaps);
                previous = i64::from(w);
            }
        }
        out.write_all(MAGIC)?;
        for block in [words.as_bytes(), &totals, &lens, &gaps, &self.counts] {
            out.write_all(&(block.len() as u32).to_le_bytes())?;
            out.write_all(block)?;
        }
        Ok(())
    }

    fn parse_packed(bytes: &[u8]) -> Result<Self, String> {
        let mut rest = bytes;
        let mut block = || -> Result<&[u8], String> {
            let (len, after) = rest.split_at_checked(4).ok_or("truncated")?;
            let len = u32::from_le_bytes(len.try_into().expect("4 bytes")) as usize;
            let (block, after) = after.split_at_checked(len).ok_or("truncated")?;
            rest = after;
            Ok(block)
        };
        let words: Vec<String> = std::str::from_utf8(block()?)
            .map_err(|e| e.to_string())?
            .lines()
            .map(str::to_string)
            .collect();
        let mut reader = Varints(block()?);
        let totals = (0..words.len())
            .map(|_| reader.next().map(|t| t as f64))
            .collect::<Option<Vec<f64>>>()
            .ok_or("short totals")?;
        let mut reader = Varints(block()?);
        let mut offsets = vec![0u32; words.len() + 1];
        for v in 0..words.len() {
            offsets[v + 1] = offsets[v] + reader.next().ok_or("short rows")? as u32;
        }
        let pairs = offsets[words.len()] as usize;
        let mut reader = Varints(block()?);
        let mut rights = Vec::with_capacity(pairs);
        for v in 0..words.len() {
            let mut previous: i64 = -1;
            for _ in offsets[v]..offsets[v + 1] {
                previous += reader.next().ok_or("short pairs")? as i64 + 1;
                rights.push(previous as u32);
            }
        }
        let counts = block()?.to_vec();
        if counts.len() != pairs {
            return Err("counts and pairs differ in number".into());
        }
        let index = words
            .iter()
            .enumerate()
            .map(|(i, w)| (w.clone(), i as u32))
            .collect();
        Ok(Self {
            words,
            index,
            totals,
            offsets,
            rights,
            counts,
        })
    }

    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// A word's index in the table.
    pub fn id(&self, word: &str) -> Option<u32> {
        self.index.get(word).copied()
    }

    pub fn total(&self, v: u32) -> f64 {
        self.totals[v as usize]
    }

    /// Word `v`'s pairs: right words (ascending) and count bytes.
    pub fn row(&self, v: u32) -> (&[u32], &[u8]) {
        let range = self.offsets[v as usize] as usize..self.offsets[v as usize + 1] as usize;
        (&self.rights[range.clone()], &self.counts[range])
    }

    /// `count(v, w)`, if the table has the pair.
    pub fn count(&self, v: u32, w: u32) -> Option<f32> {
        let (rights, counts) = self.row(v);
        rights.binary_search(&w).ok().map(|i| dequantize(counts[i]))
    }

    /// How many pairs it has.
    pub fn len(&self) -> usize {
        self.rights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rights.is_empty()
    }

    /// The table restricted to words `keep` accepts, with (when `top` is
    /// given) only its `top` pairs by `score(right word, count)`.
    pub fn prune(
        &self,
        keep: impl Fn(&str) -> bool,
        top: Option<usize>,
        score: impl Fn(&str, f32) -> f64,
    ) -> Self {
        let kept: Vec<u32> = (0..self.words.len() as u32)
            .filter(|&v| keep(&self.words[v as usize]))
            .collect();
        let mut new_id = vec![u32::MAX; self.words.len()];
        for (i, &v) in kept.iter().enumerate() {
            new_id[v as usize] = i as u32;
        }
        let mut pairs: Vec<(f64, u32, u32, u8)> = Vec::new();
        for &v in &kept {
            let (rights, counts) = self.row(v);
            for (&w, &q) in rights.iter().zip(counts) {
                if new_id[w as usize] != u32::MAX {
                    let s = score(&self.words[w as usize], dequantize(q));
                    pairs.push((s, new_id[v as usize], new_id[w as usize], q));
                }
            }
        }
        if let Some(top) = top.filter(|&t| t < pairs.len()) {
            pairs.select_nth_unstable_by(top, |a, b| b.0.total_cmp(&a.0));
            pairs.truncate(top);
        }
        Self::from_pairs(
            kept.iter()
                .map(|&v| self.words[v as usize].clone())
                .collect(),
            kept.iter().map(|&v| self.totals[v as usize]).collect(),
            pairs.into_iter().map(|(_, v, w, q)| (v, w, q)).collect(),
        )
    }
}

fn varint(mut n: u64, out: &mut Vec<u8>) {
    while n >= 0x80 {
        out.push((n as u8 & 0x7F) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

struct Varints<'a>(&'a [u8]);

impl Iterator for Varints<'_> {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        let mut n = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = self.0.split_first()?;
            self.0 = rest;
            n |= u64::from(byte & 0x7F) << shift;
            if byte < 0x80 {
                return Some(n);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_packed_agree() {
        let text = "the\t*\t1000\nthe\tsky\t40\nthe\tguy\t9\nkiss\t*\t50\nkiss\tthe\t30\n";
        let table = BigramTable::parse(text.as_bytes()).unwrap();
        let mut packed = Vec::new();
        table.write_packed(&mut packed).unwrap();
        assert!(packed.starts_with(MAGIC));
        let back = BigramTable::parse(&packed).unwrap();
        assert_eq!(back.words(), table.words());
        assert_eq!(back.len(), 3);
        let (the, sky, kiss) = (
            back.id("the").unwrap(),
            back.id("sky").unwrap(),
            back.id("kiss").unwrap(),
        );
        assert_eq!(back.total(the), 1000.0);
        // Counts survive within the byte's 6%.
        let c = back.count(the, sky).unwrap();
        assert!((c - 40.0).abs() / 40.0 < 0.07, "{c}");
        assert!(back.count(kiss, the).is_some() && back.count(sky, the).is_none());
        // Pruning to the best pair by count keeps only "the sky".
        let top = back.prune(|_| true, Some(1), |_, c| f64::from(c));
        assert_eq!(top.len(), 1);
        assert!(
            top.count(top.id("the").unwrap(), top.id("sky").unwrap())
                .is_some()
        );
    }
}
