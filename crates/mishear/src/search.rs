//! Fuzzy phonetic search: the word sequences whose pronunciations best match
//! a phoneme sequence.
//!
//! A match covers the input with words, each word aligned to a contiguous
//! span of it. Its cost is the sum, over words, of
//! - the phonetic edit cost of the word's pronunciation against its span
//!   ([`CostModel`]);
//! - `lm_weight × −ln P(word)` (common words are cheap) plus
//!   `word_penalty`;
//! - any extra cost the caller puts on particular words;
//! - any cost the caller puts on a word boundary at particular positions.
//!
//! Any alignment of the input with a concatenation of words splits into
//! per-word alignments of contiguous spans, so minimizing span by span is
//! exact. The search runs in two passes:
//! 1. For every start position, a depth-first walk of the pronunciation trie
//!    carries an edit-distance column over the input, scoring every word
//!    against every span starting there. Subtrees whose best possible cost
//!    exceeds the bound are pruned. For each span it keeps the `results`
//!    cheapest words.
//! 2. A k-best pass over the resulting lattice keeps the `results` cheapest
//!    distinct word sequences ending at each position.
//!
//! **Exactness.** Every match whose total cost is within the bound is found,
//! with its exact cost. A match within the bound has every word's cost
//! within it too, since all costs are non-negative. And keeping only the
//! `results` best words per span loses nothing from the top `results`:
//! swapping a dropped word for any of the kept ones would give that many
//! matches that are at least as good. So `search` with a bound returns the
//! top matches within that distance. [`best_matches`] gets the true top N by
//! raising the bound until the N-th match lies within it.

use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use crate::clock::Stopwatch;

use crate::cost::CostModel;
use crate::lexicon::{Lexicon, WordId};
use crate::phone::Ph;

#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// How many matches to return.
    pub results: usize,
    /// Only matches costing at most this are returned (exactly).
    pub bound: f32,
    /// Cost per nat of a word's surprisal (`−ln P(word)`).
    pub lm_weight: f32,
    /// Cost per word.
    pub word_penalty: f32,
    /// Extra cost per word, e.g. for words the input already contains.
    pub extra: HashMap<WordId, f32>,
    /// `boundary_cost[p]`: extra cost for a word boundary at input position
    /// `p` (a word starting there, `0 < p < n`). Missing positions cost
    /// nothing. Depending only on where a word starts, it leaves the search
    /// exact.
    pub boundary_cost: Vec<f32>,
    /// Score words by the previous word too, when the lexicon has bigrams
    /// (on by default). The search is then exact only up to its per-span
    /// shortlist of
    /// `shortlist` words (the best word for a span depends on its
    /// predecessor) and its cap of 400 partial matches per position.
    pub bigrams: bool,
    pub shortlist: usize,
    /// Treat hearings that sound the same (homophones spelled differently:
    /// "there" / "their") as one, keeping the cheapest spelling.
    pub distinct_sounds: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            results: 10,
            bound: 4.0,
            lm_weight: 0.005,
            word_penalty: 0.4,
            extra: HashMap::new(),
            boundary_cost: Vec::new(),
            bigrams: true,
            shortlist: 30,
            distinct_sounds: true,
        }
    }
}

/// One way to hear the input as words.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub words: Vec<WordId>,
    /// The input span each word covers.
    pub spans: Vec<Range<usize>>,
    /// Everything: phonetic + language + extra.
    pub cost: f32,
    /// The phonetic edit cost alone.
    pub phonetic: f32,
}

impl Match {
    pub fn text(&self, lexicon: &Lexicon) -> String {
        self.words
            .iter()
            .map(|&w| lexicon.word(w))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a search cost.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchStats {
    /// Trie nodes scored, summed over start positions.
    pub nodes: u64,
    /// Lattice edges (span, word) kept.
    pub edges: usize,
    /// Bounds tried by [`best_matches`] (1 for a plain search).
    pub rounds: usize,
    /// The bound of the last round.
    pub bound: f32,
    pub elapsed: Duration,
}

#[derive(Clone, Copy)]
struct Edge {
    word: WordId,
    phonetic: f32,
    /// phonetic + language + extra.
    total: f32,
}

/// The matches within `config.bound`, best first: exact (see the module
/// docs).
pub fn search(
    input: &[Ph],
    lexicon: &Lexicon,
    cost: &CostModel,
    config: &SearchConfig,
) -> (Vec<Match>, SearchStats) {
    let start = Stopwatch::start();
    let lattice = lattice(input, lexicon, cost, config, shortlist(lexicon, config, 0));
    let matches = paths(&lattice, lexicon, config, &HashMap::new());
    let stats = SearchStats {
        nodes: lattice.nodes,
        edges: lattice.edge_count,
        rounds: 1,
        bound: config.bound,
        elapsed: start.elapsed(),
    };
    (matches, stats)
}

/// How many words pass 1 must keep per span for [`paths`] to find the top
/// `config.results` exactly (with bigrams, to the search's usual
/// approximation), when up to `penalized` words may cost more there than
/// they did in pass 1.
pub fn shortlist(lexicon: &Lexicon, config: &SearchConfig, penalized: usize) -> usize {
    let keep = config.results.max(1);
    let base = if config.bigrams && lexicon.has_bigrams() {
        keep.max(config.shortlist)
    } else {
        keep
    };
    base + penalized
}

/// Pass 1's result: the best words for every span of the input.
pub struct Lattice {
    /// `edges[s][e - s - 1]`: the best words for `input[s..e]`.
    edges: Vec<Vec<Vec<Edge>>>,
    /// Nothing costing more than this was kept.
    pub bound: f32,
    /// Words kept per span, at most.
    pub keep: usize,
    /// Trie nodes scored.
    pub nodes: u64,
    /// Edges (span, word) kept.
    pub edge_count: usize,
}

/// Pass 1: for every span of `input`, its `keep` cheapest words within
/// `config.bound` (word costs include `config.extra`, and a word starting
/// after a boundary pays `config.boundary_cost` there).
pub fn lattice(
    input: &[Ph],
    lexicon: &Lexicon,
    cost: &CostModel,
    config: &SearchConfig,
    keep: usize,
) -> Lattice {
    let n = input.len();
    let bigrams = config.bigrams && lexicon.has_bigrams();
    let extra = |w: WordId| config.extra.get(&w).copied().unwrap_or(0.0);
    // With bigrams, the best case over every predecessor: a lower bound.
    let word_cost: Vec<f32> = (0..lexicon.len() as WordId)
        .map(|w| {
            let surprisal = if bigrams {
                lexicon.best_surprisal(w)
            } else {
                lexicon.surprisal(w)
            };
            config.lm_weight * surprisal + config.word_penalty + extra(w)
        })
        .collect();
    let subtree_min = subtree_min_cost(lexicon, &word_cost);
    let mut lattice = Lattice {
        edges: Vec::with_capacity(n),
        bound: config.bound,
        keep,
        nodes: 0,
        edge_count: 0,
    };
    // A word can't cover a span longer than itself plus what the bound
    // allows in dropped input.
    let extra_span = (config.bound / cost.min_indel()).floor() as usize;
    let mut present = vec![0u64; lexicon.len()];
    let mut touched: Vec<WordId> = Vec::new();
    for s in 0..n {
        let span_max = (n - s).min(lexicon.max_depth() + extra_span);
        let mut walk = Walk {
            input: &input[s..s + span_max],
            lexicon,
            cost,
            // Every word starting here pays for the boundary before it.
            bound: config.bound - boundary_cost(config, s),
            keep,
            word_cost: &word_cost,
            subtree_min: &subtree_min,
            width: span_max + 1,
            columns: vec![f32::INFINITY; (lexicon.max_depth() + 1) * (span_max + 1)],
            best: vec![Vec::new(); span_max],
            present: &mut present,
            touched: &mut touched,
            nodes: 0,
        };
        // The root column: input[s..s+k] unmatched.
        let mut acc = 0.0;
        walk.columns[0] = 0.0;
        for k in 1..=span_max {
            acc += cost.indel(input[s + k - 1]);
            walk.columns[k] = acc;
        }
        let root_min = walk.columns[..span_max + 1]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        walk.descend(0, 0, None, root_min);
        lattice.nodes += walk.nodes;
        lattice.edge_count += walk.best.iter().map(Vec::len).sum::<usize>();
        lattice.edges.push(walk.best);
        for w in touched.drain(..) {
            present[w as usize] = 0;
        }
    }
    lattice
}

/// The cost of a word boundary at `p` (none at the start).
fn boundary_cost(config: &SearchConfig, p: usize) -> f32 {
    if p == 0 {
        0.0
    } else {
        config.boundary_cost.get(p).copied().unwrap_or(0.0)
    }
}

/// Pass 2: the `config.results` best distinct word sequences through
/// `lattice` within `config.bound` (at most the lattice's), each word
/// costing `penalty` more on top of what pass 1 charged. `config` must be
/// the one the lattice was built with, but for `results` and `bound`.
pub fn paths(
    lattice: &Lattice,
    lexicon: &Lexicon,
    config: &SearchConfig,
    penalty: &HashMap<WordId, f32>,
) -> Vec<Match> {
    let edges = &lattice.edges;
    let n = edges.len();
    let bigrams = config.bigrams && lexicon.has_bigrams();
    let keep = config.results.max(1);
    let bound = config.bound.min(lattice.bound);
    let extra = |w: WordId| config.extra.get(&w).copied().unwrap_or(0.0);
    let penalty = |w: WordId| penalty.get(&w).copied().unwrap_or(0.0);
    // The `keep` best distinct word sequences ending at each position.
    let mut arena: Vec<Partial> = vec![Partial {
        cost: 0.0,
        phonetic: 0.0,
        word: WordId::MAX,
        start: 0,
        prev: u32::MAX,
        hash: 0x9E37_79B9_7F4A_7C15,
    }];
    let mut at: Vec<Vec<u32>> = vec![Vec::new(); n + 1];
    at[0].push(0);
    for e in 1..=n {
        let mut candidates: Vec<Partial> = Vec::new();
        for s in 0..e {
            let Some(span_edges) = edges[s].get(e - s - 1) else {
                continue;
            };
            let boundary = boundary_cost(config, s);
            for edge in span_edges {
                let penalty = penalty(edge.word);
                for &p in &at[s] {
                    let prev = &arena[p as usize];
                    let edge_cost = if bigrams {
                        let previous = (prev.word != WordId::MAX).then_some(prev.word);
                        edge.phonetic
                            + config.lm_weight * lexicon.surprisal_after(previous, edge.word)
                            + config.word_penalty
                            + extra(edge.word)
                    } else {
                        edge.total
                    };
                    let total = prev.cost + edge_cost + boundary + penalty;
                    if total > bound {
                        continue;
                    }
                    candidates.push(Partial {
                        cost: total,
                        phonetic: prev.phonetic + edge.phonetic,
                        word: edge.word,
                        start: s as u32,
                        prev: p,
                        hash: mix(
                            prev.hash,
                            if config.distinct_sounds {
                                lexicon.sound(edge.word)
                            } else {
                                edge.word
                            },
                        ),
                    });
                }
            }
        }
        let mut seen: Vec<u64> = Vec::new();
        // With bigrams, what a partial match costs to extend depends on its
        // last word, so the right state is (position, last word): keep the
        // best `keep` for each last word. Without, the best `keep` overall.
        let mut per_last: HashMap<WordId, usize> = HashMap::new();
        let cap = if bigrams { PARTIALS_PER_POSITION } else { keep };
        // Only the cheapest are needed: sort those first, and the rest only
        // if duplicates use them up.
        let by_cost = |a: &Partial, b: &Partial| a.cost.total_cmp(&b.cost);
        let head = (cap * 4 + 16).min(candidates.len());
        if head < candidates.len() {
            candidates.select_nth_unstable_by(head, by_cost);
        }
        candidates[..head].sort_by(by_cost);
        let mut sorted_tail = head == candidates.len();
        let mut i = 0;
        while i < candidates.len() {
            if i == head && !sorted_tail {
                candidates[head..].sort_by(by_cost);
                sorted_tail = true;
            }
            let candidate = candidates[i];
            i += 1;
            if at[e].len() == cap {
                break;
            }
            // The same words ending here with other boundaries (or, with
            // `distinct_sounds`, the same sounds spelled otherwise): the best
            // one represents them all.
            if seen.contains(&candidate.hash) {
                continue;
            }
            if bigrams {
                let count = per_last.entry(candidate.word).or_default();
                if *count == keep {
                    continue;
                }
                *count += 1;
            }
            seen.push(candidate.hash);
            at[e].push(arena.len() as u32);
            arena.push(candidate);
        }
    }

    at[n]
        .iter()
        .take(keep)
        .map(|&p| {
            let mut words = Vec::new();
            let mut spans = Vec::new();
            let mut end = n;
            let mut node = &arena[p as usize];
            let (cost, phonetic) = (node.cost, node.phonetic);
            while node.prev != u32::MAX {
                words.push(node.word);
                spans.push(node.start as usize..end);
                end = node.start as usize;
                node = &arena[node.prev as usize];
            }
            words.reverse();
            spans.reverse();
            Match {
                words,
                spans,
                cost,
                phonetic,
            }
        })
        .collect()
}

/// The true top `config.results` matches: [`search`] with the bound raised
/// (from `config.bound`, by `growth` each round, up to `max_bound`) until
/// the last of them lies within it. If even `max_bound` doesn't hold that
/// many, returns the matches within `max_bound`.
pub fn best_matches(
    input: &[Ph],
    lexicon: &Lexicon,
    cost: &CostModel,
    config: &SearchConfig,
    growth: f32,
    max_bound: f32,
) -> (Vec<Match>, SearchStats) {
    let start = Stopwatch::start();
    let mut config = config.clone();
    let mut total = SearchStats::default();
    loop {
        let (matches, stats) = search(input, lexicon, cost, &config);
        total.nodes += stats.nodes;
        total.edges = stats.edges;
        total.rounds += 1;
        total.bound = config.bound;
        if matches.len() >= config.results || config.bound >= max_bound {
            total.elapsed = start.elapsed();
            return (matches, total);
        }
        config.bound = (config.bound * growth).min(max_bound);
    }
}

/// With bigrams, at most this many partial matches end at each position.
const PARTIALS_PER_POSITION: usize = 400;

#[derive(Clone, Copy)]
struct Partial {
    cost: f32,
    phonetic: f32,
    word: WordId,
    start: u32,
    prev: u32,
    hash: u64,
}

fn mix(hash: u64, word: WordId) -> u64 {
    (hash ^ u64::from(word))
        .wrapping_mul(0x100_0000_01B3)
        .rotate_left(23)
}

/// The cheapest word cost anywhere in each node's subtree: a lower bound on
/// what finishing any word below it adds.
fn subtree_min_cost(lexicon: &Lexicon, word_cost: &[f32]) -> Vec<f32> {
    let mut min = vec![f32::INFINITY; lexicon.node_count()];
    // Children always come after their parent in the arena.
    for node in (0..lexicon.node_count()).rev() {
        let here = lexicon
            .words_at(node)
            .iter()
            .map(|&w| word_cost[w as usize])
            .fold(f32::INFINITY, f32::min);
        let below = lexicon
            .children(node)
            .iter()
            .map(|&(_, child)| min[child as usize])
            .fold(f32::INFINITY, f32::min);
        min[node] = here.min(below);
    }
    min
}

/// One start position's walk of the trie.
struct Walk<'a> {
    /// The input from the start position, as far as a word can reach.
    input: &'a [Ph],
    lexicon: &'a Lexicon,
    cost: &'a CostModel,
    bound: f32,
    keep: usize,
    word_cost: &'a [f32],
    subtree_min: &'a [f32],
    /// Column length: `input.len() + 1`.
    width: usize,
    /// One column per trie depth: `columns[d * width + k]` is the cheapest
    /// alignment of the first `k` input phonemes with the current path's
    /// first `d` phonemes.
    columns: Vec<f32>,
    /// `best[k - 1]`: the cheapest words covering the first `k` phonemes.
    best: Vec<Vec<Edge>>,
    /// `present[w]`, bit `k % 64`: `w` may be in `best[k]` (a word recurs
    /// only through its other pronunciations, so most offers skip the
    /// search for it). Cleared through `touched` after each walk.
    present: &'a mut [u64],
    touched: &'a mut Vec<WordId>,
    nodes: u64,
}

impl Walk<'_> {
    /// Scores `node`'s children. `last` is the phoneme on the edge into
    /// `node` and `node_min` the minimum of its column, for swaps.
    fn descend(&mut self, node: usize, depth: usize, last: Option<Ph>, node_min: f32) {
        let w = self.width;
        let lexicon = self.lexicon;
        for &(q, child) in lexicon.children(node) {
            let child = child as usize;
            self.nodes += 1;
            let (above, below) = self.columns.split_at_mut((depth + 1) * w);
            let parent = &above[depth * w..];
            // The column before the parent's, for a swap of the parent's
            // phoneme with this one.
            let grandparent = depth.checked_sub(1).map(|d| &above[d * w..(d + 1) * w]);
            let column = &mut below[..w];
            let indel_q = self.cost.indel(q);
            column[0] = parent[0] + indel_q;
            let mut min = column[0];
            for k in 1..w {
                let p = self.input[k - 1];
                let mut value = (parent[k - 1] + self.cost.sub(p, q))
                    .min(parent[k] + indel_q)
                    .min(column[k - 1] + self.cost.indel(p));
                if let (Some(grand), Some(l)) = (grandparent, last)
                    && k >= 2
                    && self.input[k - 2] == q
                    && p == l
                    && let Some(swap) = self.cost.swap(l, q)
                {
                    value = value.min(grand[k - 2] + swap);
                }
                column[k] = value;
                min = min.min(value);
            }
            // Below this child, a column comes from this one or, by a swap,
            // from this node's: the lower bound is the cheaper of the two.
            let below_bound = match self.cost.swap_cost() {
                Some(swap) => min.min(node_min + swap),
                None => min,
            };
            // Nothing below can finish a word within the bound.
            if below_bound.min(min) + self.subtree_min[child] > self.bound {
                continue;
            }
            for &word in lexicon.words_at(child) {
                let word_cost = self.word_cost[word as usize];
                // Span k + 1 is the first k + 1 input phonemes.
                for (k, &phonetic) in below[1..w].iter().enumerate() {
                    let total = phonetic + word_cost;
                    if total <= self.bound {
                        offer(
                            &mut self.best[k],
                            self.keep,
                            self.present,
                            self.touched,
                            k,
                            Edge {
                                word,
                                phonetic,
                                total,
                            },
                        );
                    }
                }
            }
            if depth + 1 < lexicon.max_depth() {
                self.descend(child, depth + 1, Some(q), min);
            }
        }
    }
}

/// Keeps the `keep` cheapest words for span `k` in `best`, one entry per
/// word (see [`Walk::present`]).
fn offer(
    best: &mut Vec<Edge>,
    keep: usize,
    present: &mut [u64],
    touched: &mut Vec<WordId>,
    k: usize,
    edge: Edge,
) {
    // A word already kept costs no more than the dearest kept one, so
    // anything dearer than that, when full, changes nothing.
    if best.len() == keep && best.last().is_some_and(|last| last.total <= edge.total) {
        return;
    }
    let bit = 1u64 << (k % 64);
    let mark = &mut present[edge.word as usize];
    if k >= 64 || *mark & bit != 0 {
        if let Some(known) = best.iter_mut().find(|e| e.word == edge.word) {
            if edge.total < known.total {
                *known = edge;
                best.sort_by(|a, b| a.total.total_cmp(&b.total));
            }
            return;
        }
    } else {
        if *mark == 0 {
            touched.push(edge.word);
        }
        *mark |= bit;
    }
    let at = best.partition_point(|e| e.total <= edge.total);
    best.insert(at, edge);
    best.truncate(keep);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::Frequency;
    use mime_reader::CmuDictionary;

    /// A six-word lexicon, all unigram (no bigrams).
    fn tiny() -> Lexicon {
        let dict = CmuDictionary::from_bytes(
            b"KISS  K IH1 S\nTHE  DH AH0\nTHIS  DH IH1 S\nSKY  S K AY1\nGUY  G AY1\nCASE  K EY1 S\n",
        )
        .unwrap();
        let frequencies: Vec<Frequency> = [
            ("the", 7.7),
            ("this", 6.5),
            ("guy", 5.0),
            ("case", 5.2),
            ("sky", 4.6),
            ("kiss", 4.6),
        ]
        .iter()
        .map(|&(word, zipf)| Frequency {
            word: word.into(),
            zipf,
        })
        .collect();
        Lexicon::build(&dict, &frequencies, 100, true, |_| true)
    }

    /// Every word sequence's best cost over every segmentation, by brute
    /// force: the reference the search must reproduce.
    fn brute_force(
        input: &[Ph],
        lexicon: &Lexicon,
        cost: &CostModel,
        config: &SearchConfig,
    ) -> Vec<(Vec<WordId>, f32)> {
        let pron = |w: WordId| -> Vec<Vec<Ph>> {
            let word = lexicon.word(w).to_uppercase();
            let text = format!(
                "{word}  {}",
                match word.as_str() {
                    "KISS" => "K IH1 S",
                    "THE" => "DH AH0",
                    "THIS" => "DH IH1 S",
                    "SKY" => "S K AY1",
                    "GUY" => "G AY1",
                    _ => "K EY1 S",
                }
            );
            vec![Ph::parse_all(text.split_once("  ").unwrap().1).unwrap()]
        };
        let n = input.len();
        // span_cost[(start, end, word)], computed once.
        let mut span_cost: HashMap<(usize, usize, WordId), f32> = HashMap::new();
        for start in 0..n {
            for end in start + 1..=n {
                for w in 0..lexicon.len() as WordId {
                    let phonetic = pron(w)
                        .iter()
                        .map(|p| cost.distance(&input[start..end], p))
                        .fold(f32::INFINITY, f32::min);
                    span_cost.insert(
                        (start, end, w),
                        phonetic + config.lm_weight * lexicon.surprisal(w) + config.word_penalty,
                    );
                }
            }
        }
        let boundary = |p: usize| {
            if p == 0 {
                0.0
            } else {
                config.boundary_cost.get(p).copied().unwrap_or(0.0)
            }
        };
        let mut best: HashMap<Vec<WordId>, f32> = HashMap::new();
        // Every way to cut the input into spans, and every word per span.
        for cuts in 0u32..(1 << (n - 1)) {
            let mut spans = Vec::new();
            let mut start = 0;
            for i in 1..n {
                if cuts & (1 << (i - 1)) != 0 {
                    spans.push(start..i);
                    start = i;
                }
            }
            spans.push(start..n);
            let k = spans.len();
            let words = lexicon.len();
            for choice in 0..words.pow(k as u32) {
                let mut c = choice;
                let mut seq = Vec::new();
                let mut total = 0.0;
                for span in &spans {
                    let w = (c % words) as WordId;
                    c /= words;
                    total += span_cost[&(span.start, span.end, w)] + boundary(span.start);
                    seq.push(w);
                }
                let entry = best.entry(seq).or_insert(f32::INFINITY);
                *entry = entry.min(total);
            }
        }
        let mut all: Vec<_> = best
            .into_iter()
            .filter(|(_, c)| *c <= config.bound)
            .collect();
        all.sort_by(|a, b| a.1.total_cmp(&b.1));
        all
    }

    #[test]
    fn search_matches_brute_force_within_its_bound() {
        let lexicon = tiny();
        // The default edits, and with rhythm and swaps: "K S AY1" is "sky"
        // with S K swapped, which the walk reaches only through the swap's
        // two-level step (and its pruning bound). Each also with uneven
        // boundary costs.
        let costs = [
            CostModel::default(),
            CostModel::new(crate::cost::CostConfig {
                stress_mismatch: 0.4,
                swap: Some(0.2),
                ..crate::cost::CostConfig::default()
            }),
        ];
        for (cost, (input, bound)) in costs.iter().flat_map(|c| {
            [
                ("K IH1 S DH AH0 S K AY1", 3.0),
                ("K EY1 S DH IH1 S", 4.0),
                ("G AY1 S K AY1", 5.0),
                ("K S AY1", 3.0),
                ("G AY1 K S AY1", 3.0),
                ("K IH1 K S AY1", 4.0),
            ]
            .map(|case| (c, case))
        }) {
            for boundary_cost in [vec![], vec![0.0, 0.7, 0.0, 0.4, 1.1, 0.2, 0.9, 0.3, 0.5]] {
                let input = Ph::parse_all(input).unwrap();
                let config = SearchConfig {
                    results: 5,
                    bound,
                    bigrams: false,
                    boundary_cost,
                    ..SearchConfig::default()
                };
                let expected = brute_force(&input, &lexicon, cost, &config);
                let (found, _) = search(&input, &lexicon, cost, &config);
                let want: Vec<f32> = expected.iter().take(5).map(|e| e.1).collect();
                let got: Vec<f32> = found.iter().map(|m| m.cost).collect();
                assert_eq!(
                    got.len(),
                    want.len(),
                    "{} : {got:?} vs {want:?}",
                    crate::phone::spell(&input)
                );
                for (g, w) in got.iter().zip(&want) {
                    assert!(
                        (g - w).abs() < 1e-4,
                        "{} : {got:?} vs {want:?}",
                        crate::phone::spell(&input)
                    );
                }
                assert!(found.iter().all(|m| m.cost <= bound));
            }
        }
    }

    #[test]
    fn a_swap_is_one_edit() {
        let lexicon = tiny();
        let swapping = CostModel::new(crate::cost::CostConfig {
            swap: Some(0.2),
            ..crate::cost::CostConfig::default()
        });
        let input = Ph::parse_all("K S AY1").unwrap();
        let config = SearchConfig {
            results: 1,
            bound: 3.0,
            bigrams: false,
            ..SearchConfig::default()
        };
        let (found, _) = search(&input, &lexicon, &swapping, &config);
        assert_eq!(found[0].text(&lexicon), "sky");
        assert!((found[0].phonetic - 0.2).abs() < 1e-6);
        let (plain, _) = search(&input, &lexicon, &CostModel::default(), &config);
        assert!(plain[0].phonetic > 0.2);
    }

    #[test]
    fn best_matches_raises_the_bound_until_it_has_enough() {
        let lexicon = tiny();
        let cost = CostModel::default();
        let input = Ph::parse_all("K IH1 S DH AH0 S K AY1").unwrap();
        let config = SearchConfig {
            results: 3,
            bound: 0.5,
            bigrams: false,
            ..SearchConfig::default()
        };
        let (found, stats) = best_matches(&input, &lexicon, &cost, &config, 1.5, 50.0);
        assert_eq!(found.len(), 3);
        assert!(stats.rounds > 1);
        assert_eq!(found[0].text(&lexicon), "kiss the sky");
        assert_eq!(found[0].phonetic, 0.0);
        // "kiss this guy" resegments "the sky" — the mondegreen.
        assert!(found.iter().any(|m| m.text(&lexicon) == "kiss this guy"));
    }
}
