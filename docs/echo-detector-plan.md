# Echo Detector: three phoneme-level aligners + empirical comparison

## Context

`mime-reader` scores rhyme by aligning the *rhyming parts* of two lines with a cost-minimizing OSA DP (`src/rhyme_distance.rs`). We now want **line-level phonetic similarity** ("echo") to verify repeating forms — duplex, villanelle, pantoum, blues stanza — where repeated lines are relaxed in practice: words substituted, phrases inserted ("I saw a bird—it was yellow and gold—in the forest"), lines truncated, and whole blocks re-ordered ("dark demands I call my own" ↔ "call my own dark demand sigh", which also moves a word boundary).

The rhyme DP can't do this: it's global, charges gaps per phoneme, forbids vowel indels, and has no notion of re-ordering. Three candidate alignment strategies were identified; which one best separates real echoes from unrelated lines is an empirical question. **This round builds all three behind one interface, a labeled corpus, and a comparison harness + tuning page, so the choice can be made on data.** Form verification is wired up enough to derive labeled pairs from real poems; `Poem`-level caching and wasm exposure of verification are deferred until an aligner wins.

A prior uncommitted attempt exists in another worktree; the user asked to **ignore it entirely** and design fresh.

## Decisions (fixed with the user)

| Decision | Choice |
|---|---|
| Unit of alignment | Phoneme. Lines flatten to one phoneme sequence; **boundary-blind** (word starts kept only to map results back to text). |
| Score space | Similarity (positive for alike, negative for unrelated), so local alignment works. Zero points = expected engine distance between random vowels / consonants under CMUdict, pinned as overridable constants + a test that recomputes them. Vowel never aligns with consonant. |
| Stress | Weighted on vowels: primary > secondary > unstressed; consonants flat. Echo's vowel/consonant coefficients are its own config, never read from `RhymeDistanceConfig` (its 7:1 was tuned for rhyme). |
| Gaps | Affine: `gap_open` + per-phoneme `gap_extend`. **`gap_extend` defaults to 0** — inserted material costs one event — but is configurable. |
| Re-ordering | Must be detectable, including 3+ block permutations and cross-word resegmentation. |
| B reuse (jump DP) | Tolerated by default; `reuse_penalty` configurable (0 = free). In the jump DP it is a post-hoc penalty, not a constraint — the exact no-reuse model is the Waterman–Eggert aligner. |
| Normalization | Coverage: `similarity = raw / self_similarity(shorter line)`, configurable to mean of both. `distance = 1 − similarity`. Unclamped (unrelated pairs stay continuous; clamping is presentation). |
| Pronunciation ambiguity | Enumerate per-word variant combinations up to a cap; best score over variant pairs; fall back to first pronunciation per word above the cap. |
| Use case | **Verification** of a known template with an absolute threshold tuned on the corpus — not form discovery. |
| Aligners | (1) semi-global NW–Gotoh, (3) Waterman–Eggert repeated local + masking + in-place/moved assembly, (5) jump DP. Behind one trait, selectable at runtime. |
| Transposition | Dropped for echo (block moves cover it; it complicates the 3-state kernel). `OwnedAlignmentStep::Transposition` is never emitted. |
| Deliverables | Core library + template verification + `examples/echo_compare` CLI harness + `web/tuning/echo.html` page. `Poem` integration deferred. |
| Corpus | Public-domain poems **committed** under `data/echo/public/`; copyrighted texts only in gitignored `data/echo/private/` (one tree for the loader; the user fills it by hand). |

## Architecture

Nested-module precedent in this crate is `src/pronunciation.rs` + `src/pronunciation/inflection.rs` (parent file, not `mod.rs`) — follow it.

```
src/echo.rs                    docs, `pub mod`s, re-exports, EchoConfig, EchoDetector facade, #[cfg(test)] test_support
src/echo/similarity.rs         SimilarityConfig, StressWeights, PhonemeSimilarity (69×69 table), zero-point calibration
src/echo/flatten.rs            FlatLine, flatten(), flatten_variants()
src/echo/alignment.rs          EchoAligner trait, RawAlignment, EchoAlignment, EchoStep, EchoSegment, Placement, EchoCosts, mark_in_place()
src/echo/gotoh.rs              ONE three-state affine kernel: Mode {SemiGlobal, Local, Jump}, masks, traceback → TraceEvent
src/echo/semi_global.rs        SemiGlobalAligner (1)
src/echo/waterman_eggert.rs    WatermanEggertAligner (3): mask loop + chain assembly
src/echo/jump.rs               JumpAligner (5): orientation, segments at jumps, coverage/reuse
src/echo/template.rs           EchoTemplate (duplex/villanelle/pantoum/blues/parse)
src/echo/scheme.rs             RepetitionScheme::check — verification, mirrors RhymeScheme
src/error.rs                   + EchoError, ParseEchoTemplateError
src/lib.rs                     + `pub mod echo;` + re-exports
src/wasm.rs → src/wasm/echo.rs child module: Echo DTOs + exports
examples/echo_compare/         main.rs, args.rs, corpus.rs, mutate.rs, metrics.rs, report.rs, dump.rs
data/echo/README.md, public/<form>/*.txt   committed public-domain poems;  data/echo/private/** gitignored
echo_pair_tests.json           hand-written synthetic pairs by phenomenon class (mirrors rhyme_pair_tests.json)
web/tuning/echo.html, src/echo.ts, src/config-controls.ts, src/fixtures.ts
benches/echo_align.rs          criterion, cfg-gated like benches/distance_vs_align.rs
```

### Reuse from the existing crate (don't reimplement)
- Engines: `VowelDistanceScorer::distance` / `ConsonantDistanceScorer::distance` (`src/vowel_distance.rs:78`, `src/consonant_distance.rs:50`), 0..=`NORMALIZED_DISTANCE_MAX`. `VowelHexGraph::distance` already includes a stress-mismatch penalty and is a HashMap lookup → precompute the 69×69 table once.
- Output steps: `OwnedAlignmentStep` (`src/rhyme_distance.rs:113`), so wasm's `AlignmentStepDto` (`src/wasm.rs:839`) and the tuning page's column renderer work unchanged.
- Phoneme frequencies: `PronouncingDictionary::entries()` (`src/dictionary.rs:12-48`). `VowelPhoneme`/`ConsonantPhoneme` are fieldless `Copy` enums (`as usize` indexing); `ConsonantPhoneme::ALL` is `pub(crate)`.
- Line → phonemes: `Line::words[].data` → `WordData::Known{pronunciations}` (`src/line.rs:7-60`); unknown → `UnknownWordError`. `Line::new` already splits `bird—it` at dashes, so the fixture pairs tokenize with no special handling.
- Verification pattern: `RhymeScheme`/`RhymeComparisonConfig`/`RhymeCheckResult`/`dependent_follower_indices` (`src/rhyme_scheme.rs:13-60, 231-262`).
- Test idioms: `DICT_PATH`/`OnceLock<CmuDictionary>` per module; `ph(&str)` ARPAbet helper; exhaustive-recursion DP oracle (`dp_matches_exhaustive_oracle`); "Σ step costs == score" invariant; `known_word(word, &[&[&str]])` dictionary-free `Line` builder in `rhyme_scheme.rs` tests.
- wasm: tsify DTOs with hand-written `From` impls, `set*Config(dto)` setters, `Result<_, JsError>` (`src/wasm.rs:143-146, 287-318, 389-410`). A child module `src/wasm/echo.rs` (`mod echo;` inside `wasm.rs`) can see `WasmAnalyzer`'s private fields.
- Tuning UI: `renderConfigControls` + `*_CONFIG_FIELDS`/`*_CONFIG_DOCS` + localStorage (`web/tuning/src/main.ts:31-115, 832-891`); `fixtureGroupsFrom`, `renderSectionOverview`.
- Cargo: `serde`, `serde_json` already in `Cargo.lock` via criterion → free dev-deps. `rand` is not → harness uses a tiny xorshift. `cargo test` compiles examples, and `wasm-pack test --node` therefore compiles them for wasm32 → bench-style cfg gating. `cargo run` keeps the shell cwd → resolve default paths via `env!("CARGO_MANIFEST_DIR")`.

The rhyme DP (`run_dp`, `gap_penalty`, `substitution_score`, `align`, `distance`, `step_costs`) is **not modified**. One pure extraction: move the repeated-consonant test from `gap_penalty` (`src/rhyme_distance.rs:303-316`) into `pub(crate) fn is_repeated_consonant(p: &Phoneme, prev: Option<&Phoneme>) -> bool` in `src/phoneme.rs`; `gap_penalty` calls it (behavior unchanged, covered by existing tests).

## Core library

### Config — one `EchoConfig`, three `Copy` sub-structs (all `Debug, Clone, Copy, PartialEq, Eq, Default`, doc comment per field; all numeric fields integers so the number-input UI pattern carries over)

Cost fields are in **thousandths of a unit**, `unit = sim(identical primary-stressed vowel) = vowel_coefficient × vowel_zero_point × primary_weight`, so gap/move costs stay meaningful when coefficients are retuned.

```rust
// similarity.rs
pub struct StressWeights { pub primary: u32 /*4*/, pub secondary: u32 /*3*/, pub unstressed: u32 /*2*/ }
pub struct SimilarityConfig {
    pub vowel_coefficient: u32,      // 3
    pub consonant_coefficient: u32,  // 1
    pub vowel_zero_point: u32,       // VOWEL_ZERO_POINT (pinned constant)
    pub consonant_zero_point: u32,   // CONSONANT_ZERO_POINT
    pub stress_weights: StressWeights,
}
// alignment.rs (thousandths of a unit)
pub struct GapConfig { pub gap_open: u32 /*500*/, pub gap_extend: u32 /*0*/, pub gap_open_repeated_consonant: u32 /*250*/ }
pub struct StructureConfig {
    pub move_cost: u32,     // 500  (3) per Moved run; (5) per jump
    pub run_open: u32,      // 500  (3) per run after the first
    pub min_run: u32,       // 750  (3) stop when best local run < this (one identical secondary vowel)
    pub reuse_penalty: u32, // 0    (5) per reused B phoneme, post hoc
    pub max_runs: usize,    // 16   (3) safety cap
}
// echo.rs
pub enum AlignerKind { SemiGlobal, WatermanEggert, Jump }      // Default: WatermanEggert (provisional until the comparison)
pub enum EchoNormalization { #[default] ShorterSelfSimilarity, MeanSelfSimilarity }
pub struct EchoConfig {
    pub similarity: SimilarityConfig, pub gaps: GapConfig, pub structure: StructureConfig,
    pub normalization: EchoNormalization, pub aligner: AlignerKind,
    pub max_variants_per_line: usize /*64*/, pub max_variant_pairs: usize /*256*/,
}
```

Default rationale: identical consonant ≈ 0.55 unit (vowels anchor, consonants still carry ~half a line's mass); `gap_open` ½ unit so any insertion is cheaper than one anchor; `move_cost` ½ unit (a 2-block swap of a 10-unit line lands ≈ 0.9); `run_open = gap_open` so splitting a run costs the same as gapping inside it; `min_run` ¾ unit so a lone identical consonant or unstressed vowel is chance but a lone primary vowel still counts (unrelated lines keep a small nonzero score). Flag for tuning: most consonant pairs are "unrelated" (1000), so the type-weighted consonant zero point is high and a mismatched consonant is cheap — `consonant_zero_point` is overridable for exactly this reason.

### Similarity layer (`similarity.rs`)

```rust
pub(crate) const NEG_INF: i64 = i64::MIN / 4;    // never add two sentinels; take max of candidates BEFORE adding s(i,j)
pub(crate) const PHONEME_IDS: usize = 69;         // 15 vowels × 3 stresses + 24 consonants
pub(crate) fn phoneme_id(p: &Phoneme) -> usize;
pub struct PhonemeSimilarity { table: Box<[i64]>, unit: i64, config: SimilarityConfig }
impl PhonemeSimilarity {
    pub fn new<V: VowelDistanceScorer, C: ConsonantDistanceScorer>(v: &V, c: &C, config: SimilarityConfig) -> Self;
    pub fn sim(&self, p: &Phoneme, q: &Phoneme) -> i64;   // NEG_INF across vowel/consonant
    pub fn self_similarity(&self, s: &[Phoneme]) -> i64;  // Σ sim(p,p)
    pub fn unit(&self) -> i64;
}
```
- vowels: `sim = vowel_coefficient × (vowel_zero_point − d_v(p,q)) × min(w(p.stress), w(q.stress))` — **min** guarantees `sim(p,q) ≤ min(sim(p,p), sim(q,q))`, hence normalized ≤ 1 without reuse and verbatim = exactly 1.
- consonants: `sim = consonant_coefficient × (consonant_zero_point − d_c(p,q)) × primary_weight` (flat; syllabifying consonant stress across boundaries contradicts boundary-blindness).

Zero-point calibration:
```rust
pub struct ZeroPoints { pub vowel: u32, pub consonant: u32 }
pub fn calibrate_zero_points<D: PronouncingDictionary + ?Sized, V, C>(dict: &D, v: &V, c: &C) -> ZeroPoints;
pub const VOWEL_ZERO_POINT: u32 = /* pinned */; pub const CONSONANT_ZERO_POINT: u32 = /* pinned */;
```
Count (phoneme, stress) vowel types [45] and consonants [24] over every pronunciation from `entries()` (type-weighted — CMUdict has no token frequencies; say so in the doc); `E = round(Σ n_p n_q d(p,q) / N²)` in u128. Pin test loads CMUdict and asserts `calibrate_zero_points(...) == ZeroPoints{VOWEL_ZERO_POINT, CONSONANT_ZERO_POINT}`; workflow: constants at 0 → run test → copy numbers in. Comment: recompute whenever engine config defaults change. `SimilarityConfig::default()` uses the constants so a default detector never touches the dictionary.

### Flattened line (`flatten.rs`)

```rust
pub struct FlatLine { pub phonemes: Vec<Phoneme>, pub word_starts: Vec<usize>, pub pronunciation_choice: Vec<usize> }
impl FlatLine { pub fn from_words(words: &[&[Phoneme]]) -> Self; pub fn word_at(&self, i: usize) -> usize; pub fn word_range(&self, w: usize) -> Range<usize>; len/is_empty }
pub fn flatten(line: &Line) -> Result<FlatLine, EchoError>;                                   // first pronunciation per word
pub fn flatten_variants(line: &Line, max_variants: usize) -> Result<Vec<FlatLine>, EchoError>; // mixed-radix enumeration, word 0 fastest; > cap ⇒ vec![flatten(line)?]
// error.rs
pub enum EchoError { #[error(transparent)] UnknownWord(#[from] UnknownWordError), #[error("empty line")] EmptyLine }
```
Text mapping = `line.words[flat.word_at(i)].span`; `FlatLine` does not own the `Line`.

### Aligner trait and outputs (`alignment.rs`)

```rust
pub struct EchoCosts { gap_open, gap_extend, gap_open_repeated_consonant, move_cost, run_open, min_run, reuse_penalty: i64, max_runs: usize }
impl EchoCosts { pub fn new(gaps: GapConfig, structure: StructureConfig, unit: i64) -> Self /* milli × unit / 1000 */ }
pub enum Placement { InPlace, Moved }
/// One aligned step with the indices it consumes and its signed score (sim, or −gap_open/−gap_extend).
pub struct EchoStep { pub step: OwnedAlignmentStep, pub a_index: Option<usize>, pub b_index: Option<usize>, pub score: i64 }
pub struct EchoSegment { pub a_range: Range<usize>, pub b_range: Range<usize>, pub steps: Range<usize>, pub score: i64, pub open_cost: i64, pub placement: Placement }
pub struct RawAlignment { pub raw: i64, pub steps: Vec<EchoStep>, pub segments: Vec<EchoSegment>, pub a_coverage: Vec<u8>, pub b_coverage: Vec<u8> }
impl RawAlignment { pub(crate) fn transpose(self) -> Self; pub fn reused_b(&self) -> usize /* Σ max(0, cov−1) */ }
pub trait EchoAligner {
    fn kind(&self) -> AlignerKind;
    fn is_symmetric(&self) -> bool { true }   // JumpAligner: false
    fn align(&self, a: &[Phoneme], b: &[Phoneme], sim: &PhonemeSimilarity, costs: &EchoCosts) -> RawAlignment;
}
pub struct EchoAlignment { pub raw: i64, pub similarity: f32, pub self_similarity_a: i64, pub self_similarity_b: i64, pub normalization: EchoNormalization, pub steps: Vec<EchoStep>, pub segments: Vec<EchoSegment>, pub a_coverage: Vec<u8>, pub b_coverage: Vec<u8> }
impl EchoAlignment { pub fn distance(&self) -> f32; pub fn moved_segments(&self) -> impl Iterator<Item=&EchoSegment>; pub fn reused_b(&self) -> usize }
pub(crate) fn mark_in_place(runs: &mut [EchoSegment]);   // weighted LIS on b_range.start, weight = score, O(k²)
```
Rules shared by all aligners: steps cover exactly the segments (interior gaps included), concatenated in A order, each carrying its indices and score (computed at traceback — no separate `step_costs` is needed); free leading/trailing and uncovered material are not steps — the coverage arrays describe them. Invariant: `Σ steps.score == Σ seg.score` and `raw = Σ seg.score − Σ seg.open_cost − reuse_penalty × reused_b`. Denominator 0 ⇒ similarity 0.

### Kernel (`gotoh.rs`) — one kernel, three modes

```rust
pub(crate) enum Mode { SemiGlobal, Local, Jump { move_cost: i64 } }
pub(crate) enum TraceEvent { Pair{i,j}, GapA{i}, GapB{j}, Jump }
pub(crate) fn gotoh(a, b, sim, costs, mode, mask: Option<(&[bool], &[bool])>) -> Trace { score: i64, events: Vec<TraceEvent> }
```
Notation: `s(i,j) = sim(a_i,b_j)`; `go(x, prev)` = `gap_open_repeated_consonant` if `is_repeated_consonant` else `gap_open`; `ge = gap_extend`; X = a_i unmatched, Y = b_j unmatched; `best = max(M,X,Y)`.

Common: `X[i][j] = max(M[i-1][j] − go, X[i-1][j] − ge, Y[i-1][j] − go)`; `Y[i][j] = max(M[i][j-1] − go, Y[i][j-1] − ge, X[i][j-1] − go)` (X↔Y adjacency allowed: a replaced block = two opens).

- **SemiGlobal**: `M[0][j] = M[i][0] = 0`, X/Y borders NEG_INF; `M[i][j] = s + best(i-1,j-1)`; score = `max(max_j M[n][j], max_i M[i][m])`. One prefix and one suffix free (not both sides') — the control's defining limit.
- **Local**: `M[i][j] = s + max(0, best(i-1,j-1))`; masked `(i,j)` (either index masked) ⇒ `M = X = Y = NEG_INF` — a **hard wall on all three states**, so runs are contiguous and pairwise disjoint (assembly relies on this). Score = global max of M; traceback stops at the 0 reset.
- **Jump**: `M[0][j] = 0` (free start in B); `X[1][0] = −go(a_1)`, `X[i][0] = X[i-1][0] − ge`; other borders NEG_INF. `rowbest[i-1] = max_j' best(i-1,j')` with its argmax `(j', state)` stored per row. `M[i][j] = s + max(best(i-1,j-1), rowbest[i-1] − move_cost)`, **tie → diagonal**. Score = `max_j max(M[n][j], X[n][j])` (A consumed, B suffix free). Row order: M and X for all j, then Y left→right, then rowbest. Jumps enter M only. O(nm).
- Traceback: `pred_m/x/y: Vec<u8>` with `enum Pred { Start, Reset, FromM, FromX, FromY, Jump }`; on `Jump` at `M[i][j]` continue at `rowarg[i-1]`. Deterministic tie priority: M-diag > Jump > X > Y.

### Aligners
- **(1)** `SemiGlobalAligner`: one `gotoh(SemiGlobal)` → one `InPlace` segment, `open_cost 0`.
- **(3)** `WatermanEggertAligner`: loop `gotoh(Local, mask)`; stop when `score < min_run` or `runs == max_runs`; segment = first..last consumed index per side; mask the whole `a_range`/`b_range`; sort by `a_range.start`; `mark_in_place`; `raw = Σ score − run_open × (runs − 1) − move_cost × moved`. Terminates in ≤ min(n,m) passes; ≤ 16 × 3 × 61² cell updates — microseconds.
- **(5)** `JumpAligner`: `gotoh(Jump)`; a `Jump` event closes the current segment and opens the next (first `open_cost 0`, later `move_cost`); `mark_in_place` labels segments (descriptive — the DP already charged jumps); a segment with no `Pair` has an empty `b_range`. Coverage from `Pair` events; `raw = score − reuse_penalty × reused_b` post hoc.

### Facade (`echo.rs`)

```rust
pub struct EchoDetector { similarity: PhonemeSimilarity, costs: EchoCosts, config: EchoConfig }   // NOT generic over V,C — engines only build the table
impl EchoDetector {
    pub fn new() -> Self; pub fn with_config(EchoConfig) -> Self;
    pub fn with_scorers_and_config<V, C>(v: &V, c: &C, config: EchoConfig) -> Self;
    pub fn config(&self) -> EchoConfig; pub fn set_config<V, C>(&mut self, v: &V, c: &C, config: EchoConfig);
    pub fn unit(&self) -> i64;
    pub fn compare_lines(&self, a: &Line, b: &Line) -> Result<EchoMatch, EchoError>;                        // uses config.aligner
    pub fn compare_lines_with(&self, kind: AlignerKind, a: &Line, b: &Line) -> Result<EchoMatch, EchoError>; // harness/wasm: one detector, any kind
    pub fn compare_phonemes(&self, a: &[Phoneme], b: &[Phoneme]) -> EchoAlignment;                            // dictionary-free
    pub fn compare_phonemes_with(&self, kind: AlignerKind, a: &[Phoneme], b: &[Phoneme]) -> EchoAlignment;   // runs both orientations when !is_symmetric, keeps higher normalized
}
pub struct EchoMatch { pub alignment: EchoAlignment, pub a: FlatLine, pub b: FlatLine, pub variant_pairs_tried: usize }
```
`compare_lines*`: `flatten_variants` both sides; if product > `max_variant_pairs`, truncate each list to the first ⌊√cap⌋; score every pair; return best normalized. The wasm layer builds the detector from the Analyzer's current `vowel_config()`/`consonant_config()` so the vowel/consonant tabs apply to echo too.

### Core tests (ARPAbet literals via `echo::test_support::ph`; dictionary only where stated)
Fixtures: `SWAP_A = "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N"`, `SWAP_B = "K AO1 L M AY1 OW1 N D AA1 R K D IH0 M AE1 N D S AY1"`, 3-block `X Y Z` strings, `UNRELATED_A/B`.
- similarity: `sim(AE1,AE1) == unit`; secondary/unstressed = ¾/½ unit; min rule; 69×69 symmetry; cross-class NEG_INF; `sim(M,F) < 0`; zero-point pin (dictionary).
- flatten: word_starts/word_at; "fire truck" → 2 variants (dictionary); 625 combos > 64 → 1 variant (mock dict); Unknown/Empty errors.
- gotoh: exhaustive-recursion oracle on PRNG sweeps, n,m ≤ 4, all three modes (jumps included); traced score == forward score; masked cells never appear.
- per aligner: verbatim == 1.0 exactly, one InPlace segment; Z→S exact arithmetic; 6-phoneme insertion `raw == self_short − gap_open`; truncation 1.0 ((5) via both orientations); `SWAP`: (1) < 0.7, (3) and (5) > 0.85 with exactly one Moved / one jump and exact raw; 3-block `ZXY` one move, `ZYX` two; resegmentation via `FlatLine::from_words([demands][I])` vs `([demand][sigh])` equals plain substitution; dark/darker `== 1 − gap_open/self_short`; unrelated < 0.35 for all, > 0 for (3), no NaN, empty ⇒ 0; symmetry sweeps; (3) coverage ≤ 1; (5) `"L AA1 L AA1 L AA1"` vs `"L AA1"` ⇒ `b_coverage == [3,3]`, `reused_b == 4`, penalty arithmetic.
- facade: step/segment/raw invariant on PRNG sweeps for every aligner; normalization modes; "fire truck" vs itself == 1.0 with `variant_pairs_tried == 4`; unknown word error.

## Template verification (`src/echo/template.rs`, `src/echo/scheme.rs`)

```rust
pub struct EchoTemplate { name: String, groups: Vec<Vec<usize>> /* ≥2 ascending line indices each, disjoint */, expected_line_count: Option<usize> }
impl EchoTemplate {
    pub fn from_groups(name, groups, expected_line_count) -> Result<Self, ParseEchoTemplateError>;
    pub fn duplex() -> Self;        // [0,13],[1,2],[3,4],[5,6],[7,8],[9,10],[11,12]; 14 lines
    pub fn villanelle() -> Self;    // [0,5,11,17],[2,8,14,18]; 19 lines
    pub fn pantoum(stanzas) -> Self;// k in 0..n-1: [4k+1,4k+4],[4k+3,4k+6]; closing [2,4(n-1)+1],[0,4(n-1)+3]; 4n lines
    pub fn blues(stanzas) -> Self;  // [3k,3k+1]; 3n lines
    pub fn parse(s: &str) -> Result<Self, ParseEchoTemplateError>;  // + FromStr
    pub fn pairs(&self) -> Vec<(usize,usize)>;       // (group first, each later) — RhymeScheme's leader convention
    pub fn all_pairs(&self) -> Vec<(usize,usize)>;   // every within-group i<j — corpus positives (villanelle: 12 not 6)
    pub fn name/groups/expected_line_count/max_index; pub(crate) fn group_of(&self, line) -> Option<usize>;
}
```
Text syntax: one char per line; letters label echo groups, `_` unconstrained, whitespace ignored (lay out stanzas); optional `*N` repeats the unit — under `*`, **lowercase resets per repetition** (per-stanza echoes), **uppercase is global** (refrains). Single-occurrence letters form no group; no groups ⇒ `NoEchoes`. Examples: villanelle `A_B__A__B__A__B__AB`; triolet `AB_A__AB`; blues `aab*4`; Hughes split-line blues `abab__*6`; Tennyson "Mariana" `________ABCD*7`.

```rust
pub enum ParseEchoTemplateError { Empty, InvalidChar{c, col}, InvalidRepeat{col}, NoEchoes, OverlappingGroups{index} }  // thiserror, mirrors ParseRhymeError

pub struct RepetitionScheme { template: EchoTemplate, threshold: f32 /* pass iff similarity ≥ */, aligner: AlignerKind }
pub struct EchoPairScore  { pub alignment: EchoAlignment, pub passed: bool }
pub struct EchoPairResult { pub leader: usize, pub follower: usize, pub outcome: Result<EchoPairScore, EchoError> }  // per-pair: one OOV word fails one pair, not the form
pub struct EchoFormResult { pub pairs: Vec<EchoPairResult>, pub missing_pairs: Vec<(usize,usize)> /* follower ≥ lines.len() */, pub line_count_matches: Option<bool>, pub passed: bool }
impl RepetitionScheme {
    pub fn new(template, threshold, aligner) -> Self;
    pub fn check(&self, lines: &[Line], detector: &EchoDetector) -> EchoFormResult;        // uses detector.compare_lines_with(self.aligner, ..)
    pub fn check_pair(&self, leader: &Line, follower: &Line, detector: &EchoDetector) -> Result<EchoPairScore, EchoError>;
    pub(crate) fn dependent_follower_indices(&self, line, line_count) -> Vec<usize>; pub(crate) fn leader_of(&self, line) -> Option<usize>;  // hooks for later Poem integration
}
pub const DEFAULT_ECHO_THRESHOLD: f32 = 0.5;  // placeholder until the harness picks one
```
`Poem` integration deferred: it is mechanical (a third `AnalysisState` + the two hooks above map onto `Poem::invalidate_after_update`), and doing it now would bake in the aligner and threshold this round exists to choose.

Tests (no dictionary): constructor group lists (pantoum n=2 ⇒ (1,4),(3,6),(2,5),(0,7)); parse round-trips for the examples; `aab*4` vs `AAB*4`; error columns; `pairs` vs `all_pairs` counts; `check` on a 13-line "duplex" reports `missing_pairs == [(0,13)]`; `check_pair` on an empty line ⇒ `EmptyLine`. Plus the fixture regression test (below).

## Corpus and fixtures

### Layout and file format
```
data/echo/README.md            format, licensing rule, how to add private poems, (later) chosen aligner/threshold
data/echo/public/{villanelle,pantoum,blues,refrain}/<slug>.txt    committed, public-domain only
data/echo/private/**           gitignored (.gitignore += /data/echo/private/), user-supplied copyrighted texts
```
Poem file = `key: value` header lines up to the first blank line, then the body; body blank lines are stanza breaks (kept as `stanza_starts`, excluded from line indices — `Poem` doesn't strip blanks, the loader must). Required `form`, `title`, `author`; optional `year`, `source`, `notes`, `template` (text syntax; overrides `form`; a bare `*` = repeat to fill, error if not divisible). Form → template: `villanelle` (19 lines), `duplex` (14), `pantoum` (4-line stanzas), `blues` (3-line stanzas; split-line blues use `template: abab__*`), `custom` (requires `template`). Loader = `examples/echo_compare/corpus.rs`: `CorpusPoem { id, meta, lines, stanza_starts, template }` with `parse(id, text) -> Result<_, CorpusParseError>`, `positive_pairs()` = `template.all_pairs()`, `negative_pairs()` = all i<j not sharing a group. Tests: inline parse cases per form/error, and `committed_public_corpus_is_well_formed` walking `data/echo/public/**/*.txt`. Label imbalance (villanelle: 12 positives / 159 negatives) → report balanced accuracy too; OOV lines are reported, not asserted (archaic diction expected).

### Public-domain poems to commit (fetch: Gutenberg; Gleeson White ed. *Ballades and Rondeaus…* 1887 on archive.org has most of the Victorian material)
- **villanelle/**: Dowson "Villanelle of Sunset", "…of His Lady's Treasures", "…of Marguerites" (1896), "…of Acheron", "…of the Poet's Road" (1899); Henley "Villanelle" (1877); E. A. Robinson "The House on the Hill" (cite 1894 or 1897 text) and "Villanelle of Change" (1897) — short lines all rhyming with the refrains = excellent hard negatives; Wilde "Theocritus: A Villanelle" (1881; OOV names → skipped lines); Dobson "On a Nankin Plate", "When I saw you last, Rose", "For a Copy of Theocritus"; Joyce "Villanelle of the Temptress" (1916); Gosse "Villanelle" (1877); Lang "Villanelle (To M. Joseph Boulmier)".
- **pantoum/**: Dobson "In Town" (1885) — **verify** closing-stanza order against `pantoum(n)`, use `template:` if it deviates; Brander Matthews "En Route" (1887).
- **blues/** (US-PD only — Hughes d. 1967; note in README): Hughes "The Weary Blues" (1926; custom `template:`; contains a real truncation pair), and from *Fine Clothes to the Jew* (1927): "Homesick Blues", "Po' Boy Blues", "Bound No'th Blues", "Hard Luck", "Misery", "Young Gal's Blues", "Lament over Love" (`abab__*`); W. C. Handy lyrics "St. Louis Blues" (1914), "Yellow Dog Blues", "Joe Turner Blues", "Beale Street Blues" (pre-1930). Dialect spellings are OOV → either accept skips or `notes: spelling normalized`. **Verify before committing**: "Crazy Blues" (1920), "Downhearted Blues" (1922), "See See Rider Blues" (1924/5), "Backwater Blues" (1927).
- **refrain/** (class-2 goldmine via `template:`): Tennyson "Mariana" (`________ABCD*7`, refrains vary); Shakespeare, Feste's song (`_A_B*5`); Yeats "The Stolen Child" (1889); triolets (`AB_A__AB`): Bridges "When first we met", Hardy "Birds at Winter Nightfall", "How Great My Grief", Dobson "Rose kissed me to-day", Henley "Easy is the Triolet". Optional/large: Poe "The Raven" (108 lines → gate with `--max-lines`); Kipling "Recessional" (**verify** which stanzas carry the refrain).
- **No PD examples → `private/`** (user pastes): all duplexes (Jericho Brown, 2019+); Bishop "One Art", Roethke "The Waking", Plath "Mad Girl's Love Song", Auden "If I Could Tell You", Dylan Thomas "Do not go gentle" (US copyright to 2047); Justice "Pantoum of the Great Depression", Ashbery "Pantoum". Class 10 (sparse heavy rewrite) comes **only** from the private duplexes.

### `echo_pair_tests.json` (repo root; same shape as `rhyme_pair_tests.json` + `class` and `expected: "echo"|"not"|"unlabeled"`)
```json
{ "long_insertion": { "class": 3, "expected": "echo", "comment": "One contiguous insertion or deletion.",
    "pairs": [["I saw a bird in the forest", "I saw a bird—it was yellow and gold—in the forest"],
              ["the old man sat by the fire and told us stories", "the old man told us stories"]] }, ... }
```
Groups: `verbatim`(1, incl. case/punctuation variants), `substitution`(2), `long_insertion`(3), `end_truncation`(4), `two_block_swap`(5: "I saw a bird in the forest"/"in the forest I saw a bird"…), `block_permutation`(6: "he came, he saw, he left"/"he left, he came, he saw"…), `resegmentation`(7: "dark demands I call my own"/"call my own dark demand sigh", "a nice man"/"an ice man", "the sky is grey"/"this guy is grey"), `morphological`(8), `phonetic_near`(9: "the light was thin and cold"/"the night was thin and gold"), `unrelated`(11), `hard_negative_skeleton`(11: shared function-word skeleton, "I saw a bird in the forest"/"I had a thought in the morning"), `ambiguous_rhyme_skeleton`(0, unlabeled: every content word near-rhymes its counterpart — reported, excluded from metrics). 3–4 pairs per group; all words CMUdict-pronounceable (harness reports skips).

### Mutation generator (`examples/echo_compare/mutate.rs`, not committed data)
Deterministic `XorShift64` (`--seed`, default 1); word-boundary operations on whitespace tokens of corpus lines with ≥ 6 words (≤ 3 lines per poem), each kind once → labeled positive `(original, mutant)`: `InsertPhrase` (built-in fillers, class 3), `DeleteSpan` (3), `Truncate`/`Extend` (4), `Rotate` (5), `SwapBlocks` (5), `Permute3` (6), `MorphVariant` (+s/+ed/+ing/+er, kept if `pronunciations_for` under `AllowDerived` knows it; 8). Optional: `Homophone` via `Analyzer::find_rhymes_intersection_by_words` (9) and `Shuffle` labeled unlabeled (diagnostic: which aligners treat bag-of-phonemes as echo).

## Comparison harness (`examples/echo_compare/`)

Multi-file example (auto-discovered). Every `mod` and the real `main` gated `#[cfg(not(target_arch = "wasm32"))]` + `#[cfg(target_arch = "wasm32")] fn main() {}` (same reason as the benches). Cargo.toml: `[dev-dependencies] serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`; `[[example]] name = "echo_compare" test = true` so `cargo test` runs `metrics.rs`/`corpus.rs` unit tests; `[[bench]] name = "echo_align" harness = false`.

CLI (hand-parsed `std::env::args`): `--corpus <dir>` (repeatable; default `$CARGO_MANIFEST_DIR/data/echo/public` + `private` if present), `--fixture <path>` (default `echo_pair_tests.json`), `--config <path.json>` or env `ECHO_CONFIG` (path, or inline JSON starting with `{`; an example-local all-`Option` `EchoConfigOverride` applied onto `EchoConfig::default()`, printed at startup), `--aligner semi_global|waterman_eggert|jump|all` (default all), `--no-mutations`, `--seed N`, `--max-lines N`, `--by-poem`, `--dump <path.csv|.json>`.

Pipeline: load `CmuDictionary` with `PronunciationMode::AllowDerived` → build `LabeledPair { id, source: Fixture|Corpus|Mutation, class, poem, a, b, label: Option<bool>, direct_template_pair }` from fixture + corpus positives/negatives + mutations → `Line::new` both sides, drop pairs with unknown words (print skipped counts per source + top OOV words) → for each aligner, time the loop and score every pair with `detector.compare_lines_with(kind, ..)` → report per aligner × (source, class): `n, mean, sd, min, max`; labeled-only: ROC AUC (rank-based), best-accuracy threshold (+ balanced accuracy and F1 there), margin = `min(pos) − max(neg)`, per-poem `precision@k` (k = that poem's positive count) / MRR / count of perfectly separated poems; best threshold on fixture-only vs corpus-only (does it transfer?). `--dump`: one row per pair, one column per aligner (`id,source,class,poem,label,direct,a,b,semi_global,waterman_eggert,jump`). `metrics.rs` is pure over `&[(f32, bool)]` with its own tests (perfect separation → AUC 1.0, margin > 0; reversed → 0; ties).

Fixture regression `#[test]` in `src/echo/scheme.rs` (dev-dep `serde_json`, fs dictionary like `rhyme_scheme.rs` tests): for each `AlignerKind`, every class-1 pair ≥ 0.99 and `mean(labeled positives) > mean(labeled negatives)`. After the comparison, tighten to "every labeled fixture pair is classified correctly by `AlignerKind::default()` at `DEFAULT_ECHO_THRESHOLD`".

Bench `benches/echo_align.rs`: three aligners on four fixed pairs (identical, substitution, 2-block swap, long insertion) — answers whether (3)'s repeated passes are cheap enough for per-keystroke UI.

## Tuning page and wasm exports

### wasm (`src/wasm/echo.rs`, child module; `WasmAnalyzer` gains an `EchoDetector` rebuilt on `setEchoConfig`/`setVowelConfig`/`setConsonantConfig` from the Analyzer's current engine configs)
```rust
#[wasm_bindgen(js_name = defaultEchoConfig)] pub fn default_echo_config() -> EchoConfigDto;
impl WasmAnalyzer {
    #[wasm_bindgen(js_name = echoConfig)]    pub fn echo_config(&self) -> EchoConfigDto;
    #[wasm_bindgen(js_name = setEchoConfig)] pub fn set_echo_config(&mut self, config: EchoConfigDto);
    #[wasm_bindgen(js_name = scoreEcho)]     pub fn score_echo(&self, a: &str, b: &str, aligner: AlignerKindDto) -> Result<EchoAlignmentDto, JsError>;
    #[wasm_bindgen(js_name = checkEchoForm)] pub fn check_echo_form(&self, lines: Vec<String>, template: EchoTemplateDto, threshold: f32, aligner: AlignerKindDto) -> Result<EchoFormResultDto, JsError>;
}
```
DTOs (tsify, snake_case, hand-written `From`s): `AlignerKindDto {SemiGlobal, WatermanEggert, Jump}`; `EchoNormalizationDto {Shorter, Mean}`; `EchoConfigDto` **flat** (every nested `EchoConfig` field 1:1 — flat suits `renderConfigControls`); `EchoAlignmentDto { raw, self_a, self_b, similarity, distance, a_words, b_words, a_phonemes, b_phonemes, a_word_starts, b_word_starts, steps: Vec<EchoStepDto>, segments: Vec<EchoSegmentDto>, b_coverage }`; `EchoStepDto { step: AlignmentStepDto /* existing */, score, a_index, b_index, segment: Option<usize> }`; `EchoSegmentDto { a_start, a_end, b_start, b_end, score, placement: PlacementDto }`; `EchoTemplateDto` tagged `kind`: `Duplex | Villanelle | Pantoum{stanzas} | Blues{stanzas} | Custom{scheme}`; `EchoFormResultDto { passed, line_count_matches, pairs: Vec<EchoPairResultDto /* tag status: Scored{leader, follower, passed, alignment} | Error{leader, follower, message} */>, missing_pairs: Vec<[usize;2]> }`. `#[wasm_bindgen_test]`s: identical lines ≈ 1.0 for all kinds; OOV → Err; duplex on 13 lines → `missing_pairs == [[0,13]]`, `passed == false`; `Custom{"A1"}` → Err; config round-trip.

### Web (`web/tuning`)
- `vite.config.ts`: `build.rollupOptions.input = { main: 'index.html', echo: 'echo.html' }`; nav link between pages.
- Extract from `main.ts`, behavior-preserving: `src/config-controls.ts` (`renderConfigControls(container, config, defaults, fields, docs, setValue, onChange)` with optional per-field `{min,max,step}`; `description`, `syncConfigRowModified`, `$`) and `src/fixtures.ts` (`fixtureGroupsFrom` with optional `expected`/`class`). `main.ts` keeps its behavior via `onChange`. `npm run typecheck` stays green.
- `echo.html` + `src/echo.ts`: boot like `main.ts` (`new Analyzer(bytes, "allow_derived")`, matching the harness); two line inputs + Save; threshold `number 0..1 step 0.01`; aligner radio `semi_global | waterman_eggert | jump | all` (all = three stacked result rows per pair); fixture groups from `echo_pair_tests.json` with per-aligner pass = `(similarity ≥ threshold) === (expected === 'echo')`, unlabeled groups score-only; overview chips `verbatim: SG 3/3 · WE 3/3 · J 2/3`; config aside (`ECHO_CONFIG_FIELDS`/`ECHO_CONFIG_DOCS`, normalization `<select>`, localStorage `mime_reader:tuning:echo-config:v1`, Restore defaults).
- Alignment strip per result: badge `✓ 0.83 · raw 1240 · self 1500/1480`; reuse `.align .col` (A phoneme / B phoneme / step score, match/substitution/indel tints) with a word row + dividers from `a_word_starts`, steps grouped in `<span class="segment in_place|moved">` with caption `a 3–7 → b 0–4 (+412)` (moved = distinct hue), `.unaligned` for material outside segments, `.reused` (dashed outline + count) where `b_coverage > 1`.

## Implementation order

**A. Core** (each step compiles and its tests pass before the next)
1. `error.rs` (`EchoError`, `ParseEchoTemplateError`), `phoneme.rs` (`is_repeated_consonant` extraction; rhyme tests still green), `echo.rs` skeleton, `similarity.rs` + tests; pin the zero-point constants.
2. `alignment.rs` (types, `EchoCosts`, `mark_in_place` + LIS tests), `flatten.rs` + tests.
3. `gotoh.rs` SemiGlobal + Local with oracle tests; `semi_global.rs`; (1) phenomenon tests.
4. `waterman_eggert.rs`; (3) tests incl. no-reuse and `max_runs`.
5. `Mode::Jump` (oracle extended with jumps); `jump.rs`; (5) tests incl. reuse.
6. `EchoDetector` facade, variant loop, orientation handling, invariant sweeps, `lib.rs` re-exports; `cargo build --target wasm32-unknown-unknown`.

**B. Independent of the aligners** (can run in parallel with A)
7. `template.rs` + tests; `echo_pair_tests.json`; `data/echo/README.md`; `.gitignore`; fetch + commit PD poems.
8. `examples/echo_compare/{args,corpus,mutate,metrics,report,dump}.rs` + Cargo.toml dev-deps/`[[example]]`; `committed_public_corpus_is_well_formed`.
9. Web: extract `config-controls.ts`/`fixtures.ts`, `echo.html` skeleton, vite `input`.

**C. Needs A**
10. `scheme.rs` (`RepetitionScheme`) + tests + loose fixture regression test.
11. Harness `main.rs` scoring loop; first `cargo run --release --example echo_compare`.
12. `src/wasm/echo.rs` + wasm tests; `src/echo.ts`; `benches/echo_align.rs`.

**D. After the comparison** (separate decision): set `AlignerKind::default()` and `DEFAULT_ECHO_THRESHOLD` from the harness, tighten the regression test, record the numbers in `data/echo/README.md`; then `Poem` integration + `PoemOptionsDto`.

Risks: 3-state affine traceback with free borders + 0-reset is where alignment DPs go wrong → oracle before any aligner; NEG_INF arithmetic (max before add); deterministic tie-breaking preferring diagonal over jump (else verbatim lines sprout Moved segments); hard-wall masking must wall all three states; (5) asymmetry — forgetting `transpose()` mislabels a/b (symmetry sweep catches it); variant-pair blowup in wasm is the only performance exposure (both caps are config); PD status of the 1920s blues lyrics must be verified per title before committing.

## Verification

| Command | Success |
|---|---|
| `cargo test` | all core/template/corpus/metrics tests pass; zero-point pin matches; every `data/echo/public` poem parses with the expected line count; loose fixture regression passes for all three aligners |
| `cargo run --release --example echo_compare` | prints effective config, OOV skips, per-aligner × class table, AUC / best threshold / margin / per-poem ranks; no panics |
| `cargo run --release --example echo_compare -- --dump target/echo/scores.csv --by-poem` | CSV with one column per aligner; per-poem rank table |
| `ECHO_CONFIG='{"gap_open":250}' cargo run --release --example echo_compare -- --aligner jump` | override reflected in the printed config and results |
| `cargo bench --bench echo_align` | three aligners timed on four cases |
| `cargo build --target wasm32-unknown-unknown` and `wasm-pack test --node` | existing + new `src/wasm/echo.rs` tests pass; examples compile for wasm32 via the stub `main` |
| `cd web/tuning && npm run typecheck && npm run dev` | `/` unchanged; `/echo.html` scores the fixture under all three aligners, config edits re-render, segments/moves/reuse visible |
| `cd web/tuning && npm run build` | `dist/` contains both `index.html` and `echo.html` |

Targets before choosing an aligner: fixture AUC 1.0 with positive margin; corpus AUC ≥ 0.95 at a threshold within ±0.05 of the fixture's; `precision@k` = 1.0 on the Victorian villanelles/pantoums (verbatim refrains); the informative disagreement between aligners should appear in classes 3, 5–7 and the private duplexes.
