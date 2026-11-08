# Echo corpus

Labeled data for choosing and tuning the echo aligners (`src/echo`). Scored
by `cargo run --release --example echo_compare` (see `--help`).

- `public/<form>/*.txt` — public-domain poems (US: published 1930 or
  earlier), committed.
- `private/<form>/*.txt` — copyrighted poems you paste in yourself (e.g.
  Jericho Brown's duplexes, Bishop, Roethke, Plath, Auden, Dylan Thomas,
  Justice, Ashbery). **Gitignored** — never commit texts from here. The
  harness reads it automatically when present.
- `../../echo_pair_tests.json` — synthetic pairs by phenomenon class.

## Poem file format

```
form: villanelle
title: Villanelle of Sunset
author: Ernest Dowson
year: 1896
source: https://www.gutenberg.org/ebooks/8497
notes: optional, one line
template: optional — see below
strictness: optional — strict or loose
exclude: optional — why the poem is left out of scoring

Come back to me, Sweet! ...
<line 2>
<line 3>

<stanza 2 ...>
```

`form`, `title` and `author` are required. One blank line ends the header.
`strictness` says how closely the poem's repeats match: `strict`
(near-verbatim) or `loose` (reworded). It defaults to `loose` for duplexes
and `strict` otherwise, and the harness pools and tunes by it. `exclude`
keeps a poem in the inventory but out of scoring, with the reason printed.
In the body, blank lines separate stanzas and are **not** counted: line
indices (0-based) run over non-blank lines only.

The form decides which lines echo which, after a shape check:

| form | template | shape checked |
|---|---|---|
| `villanelle` | groups [0,5,11,17], [2,8,14,18] | 19 lines; stanzas 3,3,3,3,3,4 when broken |
| `pantoum` | `EchoTemplate::pantoum(n)` | n quatrains |
| `blues` | `EchoTemplate::blues(n)` (AAB) | n tercets |
| `duplex` | [0,13], [1,2], [3,4], … [11,12] | 14 lines |
| `triolet` | `AB_A__AB` | 8 lines |
| `refrain` | — (needs `template:`) | |

A `template:` header overrides the form's: one character per line, a
letter for lines that echo each other, `_` for a free line, whitespace
ignored; `*N` repeats everything since the previous `*N`, with UPPERCASE
letters shared across repetitions and lowercase ones fresh in each (`aab*4`
is four AAB stanzas; `AAB*4` one refrain shared by all four). The template's
length must equal the poem's line count.

Positives are every pair within a template group; negatives every other
pair of lines in the same poem (a villanelle gives 12 positives and 159
negatives, so the harness reports balanced accuracy).

## Public corpus (52 poems)

Collected from Project Gutenberg (mostly Gleeson White, ed., *Ballades and
Rondeaus, Chants Royal, Sestinas, Villanelles, &c.*, 1887, #45736),
Wikisource (*Fine Clothes to the Jew*, 1927), and scans of first-edition
sheet music on Wikimedia Commons / archive.org. Each file's `source:` and
`notes:` say exactly where its text came from.

- **villanelle (16):** Dowson ×5, Henley, Dobson ×3, Gosse, E. A. Robinson
  ×2, Wilde "Theocritus", Joyce "Villanelle of the Temptress", Lang ×2 ("To
  Lucia" closes A2 A1, so it has an explicit template).
- **pantoum (2):** Dobson "In Town" (nonstandard closing stanza: explicit
  template), Brander Matthews "En Route".
- **triolet (8):** Bridges ×2, Hardy ×3 ("The Coquette, and After" is two
  triolets: `ab_a__ab*2`), Dobson ×2, Henley.
- **refrain (4):** Tennyson "Mariana", Feste's song (*Twelfth Night*), Yeats
  "The Stolen Child", Kipling "Recessional".
- **blues (22):** Hughes "The Weary Blues", "Blues Fantasy" (1926) and 15
  blues from *Fine Clothes to the Jew* (1927); W. C. Handy "St. Louis Blues"
  (1914), "Joe Turner Blues" (1915), "Beale Street Blues" (1917), "The
  Yellow Dog Rag" (1914 first edition of "Yellow Dog Blues"); Hunter/Austin
  "Down Hearted Blues" (1923). Sheet-music lyrics were transcribed from the
  scans; their line breaks follow the musical phrases.

Not included: "Crazy Blues" (only lyric-less orchestra parts found), "See
See Rider" and "Backwater Blues" (no printed lyric source found), Hughes
"Lonesome Place" (publication year unconfirmed), Poe's "The Raven" (its
refrains drift too much for a clean template).

## How the defaults were chosen

The shipped settings (`EchoConfig::for_aligner`, `EchoStrictness::threshold`)
come from the pantoums, duplexes and villanelles, public and private: 133
poems after excluding one duplex whose echoes are by meaning only. By
strictness, that is 50 strict poems (37 villanelles, 13 pantoums) and 83
loose ones (42 villanelles, 27 pantoums, 14 duplexes). Template pairs give
572 strict and 1,011 loose echoes, and every other line pair in the same
poem a non-echo (10,219 strict, 19,376 loose). 152 of the forms' 1,735
template echoes are dropped because a line has a word CMUdict lacks, which
skews the corpus toward ordinary vocabulary (the harness reports this).

**Search.** `--search 120` per aligner (run 3): random configs over every
parameter that aligner uses, screened on one fold, then refined around the
best. Poems are split into 5 folds stratified by form × strictness, and
the objective is the mean of the strict pool's AUC and the loose pool's,
so strict and loose poems count equally. Once tuned, the three aligners
then under test (semi-global, Waterman–Eggert, jump) were effectively
tied. Every search chose a much lower consonant zero point than the
calibrated 789, and mean normalization.

Run 3 reported a "nested CV estimate" for each search (semi-global 0.9934
vs 0.9896 for the defaults). **That number is optimistic**: the random
configs it chooses among were screened on fold 0, and refinement picked
its parents with the objective on every fold, held-out ones included. The
harness now computes the estimate from random trials only, on the folds
other than the screen fold. Re-running semi-global's search that way (run
4, with pronunciation variants, the setting then) shows the full search
buys nothing out of sample: its pick, chosen
without each fold and scored on it, averages 0.9945 on folds 1–4, against
0.9948 for the shipped config on the same folds. Its in-sample optimum
(0.9938 vs 0.9920) is fitted noise. The per-fold winners disagree widely
(vowel coefficient 2–6, consonant coefficient 4–6, vowel zero point
227–429, consonant zero point 171–436, gap open 589–1306, one preferring
shorter-line normalization), and run 3's optimum differed again: the
objective is flat over a wide region. So semi-global ships the hand-set
config with the two changes every search agreed on, checked as fixed
configs fold by fold (`--search 0`, no selection involved):

| semi-global config | fold 0 | fold 1 | fold 2 | fold 3 | fold 4 |
|---|---|---|---|---|---|
| old defaults (zero point 789, shorter-line) | 0.9800 | 0.9905 | 0.9940 | 0.9879 | 0.9938 |
| zero point 300, shorter-line | 0.9850 | 0.9953 | 0.9950 | 0.9917 | 0.9961 |
| zero point 789, mean | 0.9819 | 0.9930 | 0.9950 | 0.9869 | 0.9952 |
| zero point 300, mean | 0.9857 | 0.9962 | 0.9955 | 0.9914 | 0.9960 |
| **…and first pronunciations only (shipped)** | **0.9868** | **0.9967** | **0.9966** | **0.9916** | **0.9966** |

Each change helps on its own, and together they beat the old defaults on
every fold. (The first four rows compare lines over pronunciation
variants, as echo did until the last change; see Pronunciations below.) The zero point is on a plateau, not at an edge: 200, 250 and
300 all score 0.9920 overall, and 150 and 100 slightly less (the search's
range had started at 300; it now starts at 100). Waterman–Eggert's preset
is run 3's search optimum, tuned on the whole corpus — in-sample. If
anything relies on its exact values, retune it with the fixed estimator.

**Thresholds.** For loose poems, each aligner's best balanced accuracy,
which moved little when resampling poems (semi-global's best is 0.204,
90% range 0.20–0.23; 0.21 is shipped, inside it). Strict poems separate
almost perfectly anywhere in 0.38–0.55, so the strict mark sits above the
optimum, for margin.

**Pronunciations.** Each word is compared under its first pronunciation
only. Echo used to compare every combination of the words' pronunciations
(up to 64 per line and 256 pairs per comparison, both hand-set) and keep
the best-scoring pair. Measured on this corpus, that searching gave
unrelated lines more chances to match by accident: it raised 87% of
non-echo scores (by +0.025 on average) but only 41% of echo scores
(+0.023), since most echoes repeat the same words, which already share a
first pronunciation.

| semi-global | objective | loose false positives, missing 3% / 5.3% / 8.5% | time per pair (one thread) |
|---|---|---|---|
| every combination, capped at 64 / 256 | 0.9920 | 12.8% / 6.9% / 3.7% | 197 µs |
| every combination, capped at 4096 / 65,536 | 0.9918 | 13.4% / 6.9% / 3.5% | — |
| **first pronunciations only** | **0.9927** | **11.1% / 5.7% / 3.6%** | **4.3 µs** |

First pronunciations were better on every fold for semi-global and made
no difference for Waterman–Eggert (0.9923 either way), at 45–65× less
time. The cap was also inconsistent: about 8% of lines, mostly long ones,
had more combinations than it allowed and silently fell back to first
pronunciations, so they were scored by different rules from short lines.
What this gives up: an echo that only matches under an alternative
pronunciation ("live" as L AY1 V, against "alive") scores lower. If real
poems need that, it calls for choosing a word's pronunciation in a
targeted way, not a search over every combination.

**Normalization.** Both aligners divide by the mean of the two lines'
self-similarities. Dividing by the shorter line's instead makes any line
contained in another a perfect echo, however short. At equal recall, mean
normalization caught the same echoes (on loose poems 3 differed each way
out of 920; on strict poems none) with fewer false positives: 751 → 655 on
loose poems and 31 → 11 on strict ones, where the ones it removed were
mostly very short lines found inside long ones. The cost is that a
truncated or extended repeat scores below 1 — about ⅔ for a half-length
truncation, which still passes either threshold. (Measured with
pronunciation variants.)

## What ships

| | semi-global (default) | Waterman–Eggert (opt-in) |
|---|---|---|
| config | hand-set; consonant zero point 300, mean normalization | run 3's search optimum (mean normalization) |
| pronunciations | first only | first only |
| strict threshold | 0.50: 99.1% of echoes, 0.1% false positives | 0.45: 99.1%, 0.2% |
| loose threshold | 0.21: 94.3% of echoes, 5.4% false positives | 0.24: 94.1%, 5.0% |
| speed (corpus, one thread) | ~4 µs per pair | ~31 µs per pair |

False-positive rates on loose poems at equal recall (each configuration's
threshold set to miss the same share of loose echoes):

| configuration | missing 3% | missing 5.3% | missing 8.5% |
|---|---|---|---|
| semi-global, old defaults (with variants) | 22.5% | 9.9% | 5.5% |
| semi-global, zero point 300, shorter-line (with variants) | 12.4% | 7.8% | 4.0% |
| semi-global, zero point 300, mean (with variants) | 12.8% | 6.9% | 3.7% |
| **semi-global, shipped** | **11.1%** | **5.7%** | **3.6%** |
| **Waterman–Eggert, shipped** | **11.4%** | **6.1%** | **3.5%** |

Shipped against shipped, the two are within a few tenths of a point,
semi-global slightly ahead when nearly every echo must be caught. (Semi-global's run-3 search optimum looked a little better
than either — 6.0% and 3.2% at the last two columns — but that is
in-sample, and run 4 found no out-of-sample gain from the full search.) Averaging or
taking the max or min of the two aligners didn't help. So semi-global, seven
times faster and with one tuned value rather than thirteen, is the
default, and Waterman–Eggert is an alternative mode. It catches loose
echoes semi-global misses — mostly a fragment of an otherwise different
line, or a reordered phrase — at the cost of more chance fragments in
unrelated lines.

**Jump was removed.** It was never better than the other two on real
poems. Its strength, reordering, is mostly covered by Waterman–Eggert, and
its reuse of material inflated scores above 1 unless penalized.

Fixture behavior under the shipped settings is pinned by
`fixture_pairs_pass_at_the_shipped_thresholds` (`src/echo/scheme.rs`).
Semi-global's one known gap is three-block permutations (≈0.36–0.40), which
pass as loose but not as strict; Waterman–Eggert scores them 0.50–0.85.

## What the errors are

From an analysis of semi-global before the switches to mean
normalization and first pronunciations (zero point 300, shorter-line,
threshold 0.276); the shipped config misses 58 loose echoes and makes
1,041 loose false positives at 0.21.

Misses (86 of 1,011 loose echoes):

- 42: the refrain slot keeps only its end word. By the form's slots these
  are echoes; to the ear they're closer to rhyme.
- 15: one shared word elsewhere, often carried from the end of one line to
  the start of the next.
- 11: a partial phrase inside a longer line.
- 10: nothing shared in the sounds (by meaning, or spelled out).
- 8: shared words, reordered — what Waterman–Eggert is for.

False positives (773 of 19,376): 46% come from one long pantoum built on
anaphora, whose formula lines really do sound alike. The rest are real
echoes the template can't label (a line belongs to one group only), very
short lines (which mean normalization now mostly rejects), and shared
phrasing.

## Open questions

- **End-word refrains.** Relabel them, or accept a refrain slot on
  "echo, or the same end word" through the rhyme scorer.
- **Labels the template can't express** (a line echoing two groups), and
  whether one long poem should weigh as much as it does in a pooled AUC.
- **Words CMUdict lacks** drop whole refrains from the evaluation (152
  echoes); a small supplementary pronunciation list would recover them.
- **Waterman–Eggert's preset** was tuned in-sample, before a fix to how it
  assembles runs (which barely moved its scores). Semi-global's run 4
  suggests a full search's exact values don't generalize; run the same
  check for Waterman–Eggert (about a minute now) before relying on them.

## Reproducing

```sh
# the report, with pass rates at the shipped thresholds
cargo run --release --example echo_compare -- \
  --corpus data/echo/public --corpus data/echo/private \
  --forms pantoum,duplex,villanelle
# a fixed config, fold by fold (no search)
ECHO_CONFIG='{"consonant_zero_point":789,"normalization":"shorter"}' \
  cargo run --release --example echo_compare -- --search 0 --aligner semi_global \
  --corpus data/echo/public --corpus data/echo/private \
  --forms pantoum,duplex,villanelle
# the search (seconds for semi-global, about a minute for Waterman–Eggert,
# 12 threads)
cargo run --release --example echo_compare -- --search 120 \
  --corpus data/echo/public --corpus data/echo/private \
  --forms pantoum,duplex,villanelle
```

Results without the private corpus will differ: public poems are almost
all strict.
