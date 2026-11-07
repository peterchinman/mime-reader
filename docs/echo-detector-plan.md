# Echo Detector on a shared alignment engine (rhyme ported first)

## Context

`mime-reader` scores rhyme by aligning the *rhyming parts* of two lines with a cost-minimizing optimal-string-alignment DP (`RhymeDistanceScorer::run_dp`, `src/rhyme_distance.rs`). We now want **line-level phonetic similarity** ("echo") to verify repeating forms — duplex, villanelle, pantoum, blues — where repeated lines are relaxed in practice: words substituted, phrases inserted ("I saw a bird—it was yellow and gold—in the forest"), lines truncated, blocks re-ordered ("dark demands I call my own" ↔ "call my own dark demand sigh", which also moves a word boundary). Three candidate aligners will be built behind one interface and chosen **empirically** on a labeled corpus.

**Rhyme and echo share one engine.** For global alignment, cost and similarity are equivalent (Smith–Waterman–Beyer 1981): every phoneme is consumed exactly once, so with any per-phoneme mass `m`, `sim = Σm(A) + Σm(B) − cost` for every alignment — same winner, cost exactly recoverable. With `m = 0` the rhyme DP is simply the negated-cost instance of a similarity DP. A zero point (positive mass) only becomes substantive when phonemes may go unaligned for free — local, semi-global, masking, jumps — which echo needs and rhyme (pre-extracted parts, aligned completely) does not. Every other difference (gap model, transposition, weights, normalization) is a parameter. What stays distinct is the *question*: rhyme chooses what to compare by stress before aligning, wants linear per-phoneme gaps and strict order; echo lets alignment choose the region, wants one-event gaps and tolerates moves. So: **one engine, two instruments** — rhyme stays its own thing in what it compares and how it judges, not in its DP.

**Order chosen by the user: port rhyme onto the shared engine first (behavior-preserving, proven against the old DP), then build echo on it.** Coefficients remain separate per instrument (echo never reads `RhymeDistanceConfig`'s 7:1).

A prior uncommitted echo attempt exists in another worktree; the user asked to **ignore it entirely**.

## Decisions (fixed with the user)

| Decision | Choice |
|---|---|
| Engine | Shared `src/align` (score table, gap model, trace/steps, oracle). Rhyme ported first; public rhyme API, distances, alignments and thresholds unchanged. |
| Unit of alignment | Phoneme. Lines flatten to one sequence; **boundary-blind** (word starts only map results back to text). |
| Score space | Engine maximizes i64 scores. Rhyme profile: `score = −cost` (mass 0). Echo profile: similarity with calibrated zero points (positive for alike, negative for unrelated). Vowel never aligns with consonant in echo. |
| Gap model | Gap of length L scores `−(open + Σ extend(pᵢ, prevᵢ))`, `extend` class- and context-aware (repeated consonant). Rhyme: open 0, extend = today's per-phoneme penalties. Echo: open ½ unit, **extend 0 by default**, configurable. |
| Stress | Echo weights vowels primary > secondary > unstressed; consonants flat. Rhyme unchanged. |
| Re-ordering | Echo must detect it (block swaps, 3+ permutations, resegmentation). Rhyme never moves. |
| Transposition | Rhyme keeps it (linear recurrence). Echo drops it. |
| B reuse (jump DP) | Tolerated; `reuse_penalty` configurable, post hoc (exact no-reuse model = Waterman–Eggert). |
| Echo normalization | `raw / self_similarity(shorter)`, configurable to mean; unclamped; `distance = 1 − similarity`. (Rhyme's `PerMeanPhoneme` is the same measure from the cost side.) |
| Echo aligners | (1) semi-global Gotoh, (3) Waterman–Eggert repeated local + masking + in-place/moved assembly, (5) jump DP. One trait, runtime-selectable. |
| Use case | **Verification** of a known template; absolute threshold tuned on the corpus. |
| Deliverables | Part 1 rhyme port; Part 2 echo core + templates + CLI harness + `web/tuning/echo.html`. `Poem` integration deferred. |
| Corpus | Public-domain poems committed under `data/echo/public/`; copyrighted texts only in gitignored `data/echo/private/`. |

---

## Part 1 — Shared alignment engine + rhyme port (behavior-preserving)

### Layout (parent-file module style, like `src/pronunciation.rs` + `src/pronunciation/inflection.rs`)
```
src/align.rs           module docs (the cost/similarity equivalence above), NEG_INF, phoneme_id(), PHONEME_IDS = 69;
                       AlignmentStep, OwnedAlignmentStep, TranspositionDirection MOVED here from rhyme_distance.rs
                       (rhyme_distance does `pub use crate::align::{...}` so every existing path and lib.rs re-export keeps working)
src/align/table.rs     ScoreTable: 69×69 i64 in Arc<[i64]>; from_fn(|p,q| -> i64); get(id_a, id_b); ids(&[Phoneme]) -> Vec<u8>
src/align/gaps.rs      GapModel { open, open_repeated_consonant, extend_vowel, extend_consonant, extend_repeated_consonant: i64 }
                       + per-sequence precompute: GapScores { open: Vec<i64>, extend: Vec<i64> } honoring prefix_context
src/align/trace.rs     TraceEvent { Pair{i,j}, GapA{i}, GapB{j}, Swap{i,j}, Jump }, Trace { score, events };
                       events → Vec<OwnedAlignmentStep> (+ indices for echo)
src/align/linear.rs    single-state recurrence = today's run_dp, moved and negated; Global mode; optional adjacent-consonant swap;
                       trace optional; flat matrix; precomputed phoneme ids
src/align/affine.rs    three-state Gotoh (Part 2)
src/align/oracle.rs    #[cfg(test)] exhaustive-recursion oracle (today's dp_matches_exhaustive_oracle, generalized)
src/phoneme.rs         + pub(crate) fn is_repeated_consonant(p, prev) -> bool (extracted from gap_penalty, src/rhyme_distance.rs:303-316)
```

### `linear.rs` — exact generalization of `run_dp`
`pub(crate) fn align_global(a_ids, b_ids, table, gaps_a, gaps_b, swap: Option<i64>, a, b, trace: bool) -> Trace`
- `D[0][0] = 0`; `D[i][0] = D[i-1][0] + ext_a[i]`; `D[0][j]` likewise (rhyme's `open` is 0; `debug_assert!` it).
- `D[i][j] = max( D[i-1][j-1] + T[a_i][b_j], D[i][j-1] + ext_b[j], D[i-1][j] + ext_a[i], D[i-2][j-2] + swap if a_{i-1},a_i consonants ∧ a_i = b_{j-1} ∧ a_{i-1} = b_j )`.
- Ties: same priorities as `choose_edit` — match (score 0) 0 < swap 1 < costly sub 2 < GapInFirst 3 < GapInSecond 4. Each cell compares alignments of the same prefixes, so the per-cell constant offset is identical in both conventions ⇒ **identical tracebacks**, not just scores.
- `NEG_INF = i64::MIN / 4` only as the initial "best"; all candidates are finite (no saturation needed). Rhyme returns `u32::try_from(-score).unwrap_or(u32::MAX)` (old saturating behavior at the limit).

### `RhymeDistanceScorer` port (`src/rhyme_distance.rs`)
- Keep generic `V, C`, all constructors, getters/setters, `RhymeDistanceConfig`, `distance`/`align`/`step_costs`, prefix stripping and the shorter-first swap in `distance`.
- New fields: `table: ScoreTable` (entries `−engine × coefficient`, cross-class `−vowel_to_consonant_mismatch`) and `gaps: GapModel` (open 0; extend `−vowel_indel_penalty` / `−consonant_indel_penalty` / `−consonant_repeated_penalty`). Rebuilt in `with_*`, `set_config`, `set_vowel_config`, `set_consonant_config` (4,761 engine calls — microseconds). `Arc` keeps `Clone` cheap (wasm `scoreRhyme` clones per call).
- `run_dp`/`backtrack_alignment` replaced by `linear::align_global` + trace→`AlignmentStep<'a>` (Match if equal else Substitution; Indel; Transposition Left/Right exactly as today). `substitution_score`/`gap_penalty` become lookups (negated) so `step_costs` is unchanged in behavior.
- Exactness precondition verified: every `Consonant` is built via `From<ConsonantPhoneme>` (`src/phoneme.rs:150-189`; test helpers at `src/rhyme_distance.rs:795`, `src/consonant_distance.rs:181` parse or call `From`), and `Vowel` = (phoneme, stress), so a per-phoneme table equals per-cell engine calls. Mock engines (`FixedVowelScorer`, `FixedConsonantScorer`, `src/rhyme_distance.rs:764-772`) feed the table the same way.

### Proof of equivalence
- Copy today's `run_dp` + `backtrack_alignment` verbatim into `#[cfg(test)] mod legacy` in `rhyme_distance.rs` (lands with the port PR; removed in the first echo PR).
- `port_matches_legacy_on_random_sequences`: PRNG sweep, lengths 0..=7, alphabet with stressed vowels, repeated consonants and swap pairs; default config, randomized configs, and the mock engines; assert equal score **and** equal `Vec<OwnedAlignmentStep>`.
- `port_matches_legacy_on_dictionary_rhyming_parts`: deterministic sample (sort `entries()` first — HashMap order is unspecified) of 2,000 word pairs' rhyming parts from CMUdict; equal distance and alignment.
- All existing tests unchanged and green: `score_equals_summed_alignment_cost`, `dp_matches_exhaustive_oracle`, transposition/repeated-consonant/symmetry tests, `rhyme_scheme`/`poem`/`analyzer` tests, 25 `#[wasm_bindgen_test]`s (alignment shapes).
- Perf gate: criterion baseline before/after on `find_rhymes` and `distance_vs_align`; no significant regression > 5% (table lookup is expected to beat the HashMap lookup in `VowelHexGraph`).
- Tuning page: section-overview pass counts identical before/after.

### Follow-ups enabled (not this round)
- Rhyme's open TODO "compare different sized syllable slices — find the smaller-sized slice of the larger one with the lowest distance score" = semi-global mode; it needs **rhyme's own** zero point (with mass 0, free ends would skip everything). Separate design decision.
- Identical-rhyme handling (bear/bare scores perfect today) and mosaic rhymes via boundary-blind flattening.

---

## Part 2 — Echo detector on the shared engine

### Layout
```
src/align/affine.rs            three-state Gotoh: Mode { Global, SemiGlobal, Local, Jump { move_cost } }, optional masks
src/echo.rs                    docs, pub mods, re-exports, EchoConfig, EchoDetector facade, #[cfg(test)] test_support
src/echo/similarity.rs         SimilarityConfig, StressWeights, zero points + calibration; builds an align::ScoreTable + unit
src/echo/flatten.rs            FlatLine, flatten(), flatten_variants()
src/echo/alignment.rs          EchoAligner trait, RawAlignment, EchoAlignment, EchoStep, EchoSegment, Placement, EchoCosts, mark_in_place()
src/echo/semi_global.rs        SemiGlobalAligner (1)
src/echo/waterman_eggert.rs    WatermanEggertAligner (3)
src/echo/jump.rs               JumpAligner (5)
src/echo/template.rs           EchoTemplate;  src/echo/scheme.rs  RepetitionScheme
src/error.rs                   + EchoError, ParseEchoTemplateError
src/wasm/echo.rs               child module of wasm.rs: Echo DTOs + exports
examples/echo_compare/         main.rs, args.rs, corpus.rs, mutate.rs, metrics.rs, report.rs, dump.rs
data/echo/{README.md, public/<form>/*.txt}; data/echo/private/** gitignored
echo_pair_tests.json; web/tuning/{echo.html, src/echo.ts, src/config-controls.ts, src/fixtures.ts}; benches/echo_align.rs
```

### Reuse
- From Part 1: `ScoreTable`, `GapModel`/`GapScores`, `TraceEvent`→steps, `OwnedAlignmentStep`, `is_repeated_consonant`, oracle harness.
- Engines `VowelDistanceScorer`/`ConsonantDistanceScorer` (`src/vowel_distance.rs:78`, `src/consonant_distance.rs:50`); `PronouncingDictionary::entries()` for calibration; `Line`/`WordData` (`src/line.rs`); `RhymeScheme` patterns (`src/rhyme_scheme.rs`); wasm DTO conventions (`src/wasm.rs:143-146, 287-318, 389-410, 839`); tuning UI `renderConfigControls`/`fixtureGroupsFrom` (`web/tuning/src/main.ts`). `Line::new` already splits `bird—it` at dashes.
- Cargo: `serde`/`serde_json` already in `Cargo.lock` → free dev-deps; no `rand` (use xorshift); examples need the wasm32 `fn main(){}` stub; resolve paths with `env!("CARGO_MANIFEST_DIR")`.

### Echo config (integers; costs in thousandths of `unit = sim(identical primary vowel) = vowel_coefficient × vowel_zero_point × primary_weight`)
```rust
pub struct StressWeights { pub primary: u32 /*4*/, pub secondary: u32 /*3*/, pub unstressed: u32 /*2*/ }
pub struct SimilarityConfig { pub vowel_coefficient: u32 /*3*/, pub consonant_coefficient: u32 /*1*/,
    pub vowel_zero_point: u32 /*VOWEL_ZERO_POINT*/, pub consonant_zero_point: u32 /*CONSONANT_ZERO_POINT*/, pub stress_weights: StressWeights }
pub struct GapConfig { pub gap_open: u32 /*500*/, pub gap_extend: u32 /*0*/, pub gap_open_repeated_consonant: u32 /*250*/ }
pub struct StructureConfig { pub move_cost: u32 /*500*/, pub run_open: u32 /*500*/, pub min_run: u32 /*750*/,
    pub reuse_penalty: u32 /*0*/, pub max_runs: usize /*16*/ }
pub enum AlignerKind { SemiGlobal, WatermanEggert /*provisional default*/, Jump }
pub enum EchoNormalization { #[default] ShorterSelfSimilarity, MeanSelfSimilarity }
pub struct EchoConfig { similarity, gaps, structure, normalization, aligner, max_variants_per_line: usize /*64*/, max_variant_pairs: usize /*256*/ }
```
Rationale: identical consonant ≈ 0.55 unit; any insertion (½) cheaper than one anchor; a 2-block swap of a 10-unit line ≈ 0.9; `run_open = gap_open`; `min_run` ¾ unit (lone primary vowel counts, lone consonant doesn't). Tuning flag: high type-weighted consonant zero point makes mismatched consonants cheap — overridable.

### Similarity table (`echo/similarity.rs`)
- vowels: `coeff_v × (z_v − d_v(p,q)) × min(w(p), w(q))` — **min** ⇒ `sim(p,q) ≤ min(sim(p,p), sim(q,q))` ⇒ normalized ≤ 1 without reuse, verbatim = 1 exactly. Consonants: `coeff_c × (z_c − d_c) × primary_weight` (flat). Cross-class: `NEG_INF`.
- `calibrate_zero_points(dict, v, c) -> ZeroPoints`: type-weighted counts over all pronunciations in `entries()` (45 vowel types incl. stress, 24 consonants); `E = round(Σ n_p n_q d(p,q) / N²)` in u128. Pinned `VOWEL_ZERO_POINT`/`CONSONANT_ZERO_POINT` + a CMUdict test that recomputes them (constants at 0 → run → copy).

### Flattening (`echo/flatten.rs`)
`FlatLine { phonemes, word_starts, pronunciation_choice }`, `from_words`, `word_at`, `word_range`; `flatten(&Line)` (first pronunciation per word); `flatten_variants(&Line, cap)` (mixed radix, word 0 fastest; > cap ⇒ first-pronunciation only). `EchoError { UnknownWord(#[from] UnknownWordError), EmptyLine }`.

### Trait and outputs (`echo/alignment.rs`)
```rust
pub struct EchoStep { pub step: OwnedAlignmentStep, pub a_index: Option<usize>, pub b_index: Option<usize>, pub score: i64 }
pub struct EchoSegment { pub a_range: Range<usize>, pub b_range: Range<usize>, pub steps: Range<usize>, pub score: i64, pub open_cost: i64, pub placement: Placement }
pub struct RawAlignment { pub raw: i64, pub steps: Vec<EchoStep>, pub segments: Vec<EchoSegment>, pub a_coverage: Vec<u8>, pub b_coverage: Vec<u8> }
pub trait EchoAligner { fn kind(&self) -> AlignerKind; fn is_symmetric(&self) -> bool { true }
    fn align(&self, a: &[Phoneme], b: &[Phoneme], table: &ScoreTable, costs: &EchoCosts) -> RawAlignment; }
pub struct EchoAlignment { raw, similarity: f32, self_similarity_a, self_similarity_b, normalization, steps, segments, a_coverage, b_coverage }  // + distance(), moved_segments(), reused_b()
```
Invariants: steps cover exactly the segments in A order; `Σ steps.score == Σ seg.score`; `raw = Σ seg.score − Σ seg.open_cost − reuse_penalty × reused_b`; denominator 0 ⇒ similarity 0.

### `align/affine.rs` — three-state Gotoh (gap of L = `open + Σ extend`, matching Part 1's model)
`go(i)` = open (repeated-consonant variant), `ge(i)` = per-position extend.
- `X[i][j] = max(M[i-1][j] − go(i) − ge(i), X[i-1][j] − ge(i), Y[i-1][j] − go(i) − ge(i))`; `Y` symmetric.
- **Global**: borders = cumulative gaps; end at (n,m). Exists for the cross-check below.
- **SemiGlobal**: `M[0][j] = M[i][0] = 0`; `M = s + best(i-1,j-1)`; score = max over last row/column of M.
- **Local** (+ mask): `M = s + max(0, best(i-1,j-1))`; masked cell ⇒ `M = X = Y = NEG_INF` (hard wall on all three states ⇒ runs contiguous and disjoint).
- **Jump**: `M[0][j] = 0`; X border from cumulative gaps; `M = s + max(best(i-1,j-1), rowbest[i-1] − move_cost)`, tie → diagonal; score = `max_j max(M[n][j], X[n][j])`; O(nm) via per-row argmax.
- Traceback via `pred_m/x/y` (`Start, Reset, FromM, FromX, FromY, Jump`); tie priority M-diag > Jump > X > Y. Emits Part 1's `TraceEvent`s.
- **Cross-check test**: `affine(Global, open = 0)` score == `linear::align_global(no swap)` on PRNG sweeps.

### Aligners
- (1) one `affine(SemiGlobal)` → one InPlace segment.
- (3) loop `affine(Local, mask)` until `score < min_run` or `max_runs`; mask each run's full ranges; sort by `a_range.start`; `mark_in_place` (weighted LIS on `b_range.start`); `raw = Σ score − run_open × (runs − 1) − move_cost × moved`.
- (5) `affine(Jump)`; jumps delimit segments (later ones `open_cost = move_cost`); `mark_in_place` labels; coverage from `Pair`s; `raw = score − reuse_penalty × reused_b`.

### Facade (`echo.rs`)
`EchoDetector { table, unit, costs, config }` (not generic; engines only build the table): `new`, `with_config`, `with_scorers_and_config(v, c, config)`, `set_config(v, c, config)`, `compare_lines`, `compare_lines_with(kind, ..)`, `compare_phonemes`, `compare_phonemes_with(kind, ..)` (runs both orientations when `!is_symmetric`, keeps higher normalized). `compare_lines*` enumerates variants (both caps), returns best `EchoMatch { alignment, a: FlatLine, b: FlatLine, variant_pairs_tried }`. wasm builds it from the Analyzer's current vowel/consonant configs.

### Core tests (ARPAbet literals; dictionary only where stated)
Fixtures `SWAP_A = "D AA1 R K D IH0 M AE1 N D Z AY1 K AO1 L M AY1 OW1 N"`, `SWAP_B = "K AO1 L M AY1 OW1 N D AA1 R K D IH0 M AE1 N D S AY1"`, 3-block strings, `UNRELATED_A/B`.
- similarity: unit / ¾ / ½ for stresses; min rule; symmetry; cross-class NEG_INF; `sim(M,F) < 0`; zero-point pin (dictionary).
- flatten: word_starts/word_at; "fire truck" → 2 variants; mock 625 > 64 → 1; errors.
- affine: oracle sweeps (n,m ≤ 4) for all four modes; traced == forward score; masked cells never appear; the Global/linear cross-check.
- per aligner: verbatim == 1.0, one InPlace segment; Z→S exact; 6-phoneme insertion `raw == self_short − gap_open`; truncation 1.0; SWAP: (1) < 0.7, (3)/(5) > 0.85 with exactly one Moved/jump and exact raw; `ZXY` one move, `ZYX` two; resegmentation (`[demands][I]` vs `[demand][sigh]`) == plain substitution; dark/darker exact; unrelated < 0.35 all, > 0 for (3); empty ⇒ 0; symmetry; (3) coverage ≤ 1; (5) `"L AA1 L AA1 L AA1"` vs `"L AA1"` ⇒ `b_coverage [3,3]`, `reused_b 4`, penalty arithmetic.
- facade: invariants on sweeps for every aligner; normalization modes; "fire truck" vs itself == 1.0 with 4 variant pairs; unknown word error.

### Template verification (`echo/template.rs`, `echo/scheme.rs`)
- `EchoTemplate { name, groups: Vec<Vec<usize>>, expected_line_count }`: `duplex()` [0,13],[1,2],[3,4],[5,6],[7,8],[9,10],[11,12]; `villanelle()` [0,5,11,17],[2,8,14,18]; `pantoum(n)` [4k+1,4k+4],[4k+3,4k+6] for k < n−1, closing [2,4(n−1)+1],[0,4(n−1)+3]; `blues(n)` [3k,3k+1]; `parse` (letters = groups, `_` free, whitespace ignored, `*N` repeat with lowercase-per-repetition / uppercase-global; e.g. `A_B__A__B__A__B__AB`, `AB_A__AB`, `aab*4`, `abab__*6`, `________ABCD*7`); `pairs()` (leader = group's first line), `all_pairs()` (corpus positives).
- `ParseEchoTemplateError { Empty, InvalidChar{c,col}, InvalidRepeat{col}, NoEchoes, OverlappingGroups{index} }`.
- `RepetitionScheme { template, threshold, aligner }` → `check(&[Line], &EchoDetector) -> EchoFormResult { pairs: Vec<EchoPairResult { leader, follower, outcome: Result<EchoPairScore{alignment, passed}, EchoError> }>, missing_pairs, line_count_matches, passed }`; `check_pair`; `pub(crate)` `leader_of` / `dependent_follower_indices` for later `Poem` integration. `DEFAULT_ECHO_THRESHOLD = 0.5` placeholder.
- Tests: constructor groups (pantoum n=2 ⇒ (1,4),(3,6),(2,5),(0,7)); parse round-trips; `aab*4` vs `AAB*4`; error columns; 13-line duplex ⇒ `missing_pairs == [(0,13)]`; empty line ⇒ `EmptyLine`.

### Corpus and fixtures
- Poem file: `key: value` header (`form`, `title`, `author` required; `year`, `source`, `notes`, `template` optional) + blank line + body; body blank lines = stanza breaks (excluded from indices). Form → template with line-count / stanza-shape checks. Loader in `examples/echo_compare/corpus.rs`: positives = `all_pairs()`, negatives = all other i<j in the same poem; `committed_public_corpus_is_well_formed` test. Report balanced accuracy (villanelle: 12 pos / 159 neg).
- **Public domain to commit** (Gutenberg; Gleeson White ed., *Ballades and Rondeaus…*, 1887): villanelles — Dowson (5: Sunset, His Lady's Treasures, Marguerites, Acheron, Poet's Road), Henley "Villanelle", E. A. Robinson "The House on the Hill" (cite text year) + "Villanelle of Change", Wilde "Theocritus", Dobson (On a Nankin Plate; When I saw you last, Rose; For a Copy of Theocritus), Joyce "Villanelle of the Temptress" (1916), Gosse, Lang; pantoums — Dobson "In Town" (**verify** closing order), Brander Matthews "En Route"; blues (US-PD only) — Hughes "The Weary Blues" (1926) + *Fine Clothes to the Jew* (1927) blues, Handy lyrics (St. Louis, Yellow Dog, Joe Turner, Beale Street); **verify** Crazy Blues / Downhearted Blues / See See Rider / Backwater Blues; refrain forms — Tennyson "Mariana", Feste's song, Yeats "The Stolen Child", triolets (Bridges, Hardy ×2, Dobson, Henley); optional Poe "The Raven" (`--max-lines`), Kipling "Recessional" (**verify**).
- **Private only** (user pastes): Jericho Brown duplexes (sole source of class 10), Bishop, Roethke, Plath, Auden, Dylan Thomas, Justice, Ashbery.
- `echo_pair_tests.json` (root; rhyme fixture shape + `class`, `expected: "echo"|"not"|"unlabeled"`): verbatim(1), substitution(2), long_insertion(3, incl. the bird/forest pair), end_truncation(4), two_block_swap(5), block_permutation(6), resegmentation(7, incl. "dark demands I call my own"/"call my own dark demand sigh", "a nice man"/"an ice man"), morphological(8), phonetic_near(9), unrelated(11), hard_negative_skeleton(11), ambiguous_rhyme_skeleton(0, unlabeled). 3–4 pairs each, all CMUdict-pronounceable.
- Mutation generator (`examples/echo_compare/mutate.rs`, xorshift, `--seed`): InsertPhrase/DeleteSpan (3), Truncate/Extend (4), Rotate/SwapBlocks (5), Permute3 (6), MorphVariant (8); optional Homophone (9), Shuffle (unlabeled diagnostic).

### Comparison harness (`examples/echo_compare/`)
`[[example]] name = "echo_compare" test = true`; dev-deps `serde` (derive), `serde_json`; wasm32 stub `main`. CLI: `--corpus` (repeatable; default public + private if present), `--fixture`, `--config path.json` / `ECHO_CONFIG` (all-`Option` override, printed at startup), `--aligner semi_global|waterman_eggert|jump|all`, `--no-mutations`, `--seed`, `--max-lines`, `--by-poem`, `--dump csv|json`. Pipeline: dictionary (`AllowDerived`) → labeled pairs → drop OOV (report counts + top OOV words) → score with each aligner (timed) → per aligner × (source, class) `n/mean/sd/min/max`; ROC AUC, best threshold (+ balanced accuracy, F1), margin, per-poem precision@k / MRR / perfectly-separated count, fixture-vs-corpus threshold transfer. Fixture regression test in `echo/scheme.rs`: every class-1 pair ≥ 0.99 and mean(pos) > mean(neg) for every aligner (tightened after the comparison). Bench `benches/echo_align.rs`.

### Tuning page and wasm
- `src/wasm/echo.rs`: `defaultEchoConfig()`, `echoConfig()`/`setEchoConfig(EchoConfigDto)` (flat DTO), `scoreEcho(a, b, AlignerKindDto) -> Result<EchoAlignmentDto, JsError>`, `checkEchoForm(lines, EchoTemplateDto, threshold, aligner)`. DTOs: `EchoAlignmentDto { raw, self_a, self_b, similarity, distance, a_words, b_words, a_phonemes, b_phonemes, a_word_starts, b_word_starts, steps: Vec<EchoStepDto { step: AlignmentStepDto, score, a_index, b_index, segment }>, segments: Vec<EchoSegmentDto { a_start, a_end, b_start, b_end, score, placement }>, b_coverage }`; `EchoTemplateDto` tagged `Duplex | Villanelle | Pantoum{stanzas} | Blues{stanzas} | Custom{scheme}`; `EchoFormResultDto` with `Scored | Error` pair results. wasm tests: identical ≈ 1.0 per kind; OOV → Err; 13-line duplex missing `[0,13]`; bad custom → Err; config round-trip.
- Web: vite `build.rollupOptions.input = { main, echo }`; extract `config-controls.ts` and `fixtures.ts` from `main.ts` (behavior-preserving); `echo.html` + `src/echo.ts` with two line inputs, threshold 0..1, aligner radio incl. "all", fixture groups with per-aligner pass chips, echo config aside (localStorage `mime_reader:tuning:echo-config:v1`), alignment strip with word row, segment spans (in_place / moved), `.unaligned`, `.reused`.

---

## Implementation order

**Part 1 — rhyme port** (one PR, behavior-preserving)
1. Baselines on current HEAD: `cargo bench --bench find_rhymes -- --save-baseline pre-align`; same for `distance_vs_align`.
2. `src/align.rs` skeleton; move step types (re-exported from `rhyme_distance`); extract `is_repeated_consonant`. `cargo test` green.
3. `table.rs`, `gaps.rs`, `trace.rs` + unit tests.
4. `linear.rs` + oracle (moved from `rhyme_distance` tests, generalized).
5. `mod legacy` copy; switch `RhymeDistanceScorer` to table + gaps + `linear::align_global`; equivalence tests (random + dictionary); full `cargo test`; `wasm-pack test --node`.
6. Benches vs baseline; tuning-page counts unchanged.

**Part 2 — echo** (A and B in parallel; C after A)
- A. `affine.rs` (Global + SemiGlobal + Local, oracle, cross-check) → `echo/similarity.rs` (pin zero points) → `alignment.rs` + `flatten.rs` → (1) → (3) → Jump mode + (5) → facade + re-exports; `cargo build --target wasm32-unknown-unknown`; remove `mod legacy`.
- B. `template.rs` + tests; `echo_pair_tests.json`; `data/echo/README.md`; `.gitignore` += `/data/echo/private/`; PD poems; harness `args/corpus/mutate/metrics/report/dump`; web extraction + `echo.html` skeleton.
- C. `scheme.rs` + loose fixture regression; harness scoring loop + first run; `src/wasm/echo.rs` + `echo.ts`; `benches/echo_align.rs`.
- D (after the comparison, separate decision): set default aligner + threshold, tighten regression test, record numbers in `data/echo/README.md`; then `Poem` integration; then rhyme follow-ups (semi-global with rhyme's own zero point).

Housekeeping: once approved, replace `docs/echo-detector-plan.md` on `peter/echo-detector-plan` with this revision and push.

Risks: tie-breaking must be preserved exactly in `linear.rs` (equivalence test compares steps, not just scores); table rebuild must happen in every config/engine setter (test: `set_vowel_config` changes `distance`); `find_rhymes` perf (i64 + flat matrix + table vs u32 + HashMap — measured, not assumed); affine traceback with free borders + 0-reset (oracle first); NEG_INF arithmetic (max before add); diagonal-over-jump ties (else verbatim lines sprout Moved segments); masks must wall all three states; jump asymmetry (`transpose()`, symmetry sweep); variant-pair blowup in wasm (caps); PD status of 1920s blues lyrics per title.

## Verification

| Command | Success |
|---|---|
| `cargo test` (after Part 1) | all pre-existing tests unchanged and green; `port_matches_legacy_*` green (identical scores **and** steps) |
| `cargo bench --bench find_rhymes -- --baseline pre-align` (+ `distance_vs_align`) | no significant regression > 5% |
| `wasm-pack test --node` (after Part 1) | 25 existing wasm tests green, unchanged |
| `cd web/tuning && npm run dev` (after Part 1) | rhyme page section-overview counts identical to before |
| `cargo test` (after Part 2) | align/echo/template/corpus/metrics tests green; zero-point pin matches; affine↔linear cross-check; every public poem parses; loose fixture regression passes for all three aligners |
| `cargo run --release --example echo_compare` | effective config, OOV skips, per-aligner × class table, AUC / threshold / margin / per-poem ranks; no panics |
| `cargo run --release --example echo_compare -- --dump target/echo/scores.csv --by-poem` | one column per aligner; per-poem rank table |
| `ECHO_CONFIG='{"gap_open":250}' cargo run --release --example echo_compare -- --aligner jump` | override reflected |
| `cargo bench --bench echo_align` | three aligners timed on four cases |
| `cargo build --target wasm32-unknown-unknown` && `wasm-pack test --node` | existing + echo wasm tests green; examples compile for wasm32 |
| `cd web/tuning && npm run typecheck && npm run build && npm run dev` | `/` unchanged; `/echo.html` scores fixtures under all three aligners with segments/moves/reuse visible; `dist/` has both pages |

Targets before choosing an aligner: fixture AUC 1.0 with positive margin; corpus AUC ≥ 0.95 at a threshold within ±0.05 of the fixture's; precision@k = 1.0 on the Victorian villanelles/pantoums; the informative disagreements in classes 3, 5–7 and the private duplexes.
