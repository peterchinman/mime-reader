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
poem a non-echo (10,219 strict, 19,376 loose).

**Search.** `--search 120` per aligner: random configs over every parameter
that aligner uses, screened on one fold, then refined around the best.
Poems are split into 5 folds stratified by form × strictness, and the
objective is the mean of the strict pool's AUC and the loose pool's, so
strict and loose poems count equally. A nested estimate (best config
picked without each fold, scored on it) checks that tuning generalizes:

| aligner | defaults | nested CV estimate |
|---|---|---|
| semi-global | 0.9896 | 0.9934 |
| Waterman–Eggert | 0.9899 | 0.9931 |
| jump (since removed) | 0.9826 | 0.9917 |

The three were effectively tied once tuned. Two findings held for all of
them:

- **The consonant zero point belongs well below chance.** CMUdict
  calibration puts it at 789, the mean distance between random
  consonants. Every search preferred 300–540. Lowering it alone to 300
  took semi-global from 0.9883 to 0.9918 (the full optimum: 0.9931), and
  jump from 0.9819 to 0.9905. Waterman–Eggert barely moved (0.9891): it
  also needs its structural costs tuned.
- **Mean normalization was always chosen.** It stops short lines that
  happen to sit inside longer ones from scoring as full echoes. Vowel-side
  values (coefficients, vowel zero point, stress weights) did not agree
  across folds.

**Thresholds.** For loose poems, each aligner's best balanced accuracy,
which moved little when resampling poems (semi-global's 90% range:
0.22–0.30). Strict poems separate almost perfectly anywhere in
0.41–0.68, so the strict mark sits above the optimum, for margin.

## What ships

| | semi-global (default) | Waterman–Eggert (opt-in) |
|---|---|---|
| config | defaults; consonant zero point 300 | the search's optimum (mean normalization) |
| strict threshold | 0.50: 99.1% of echoes, 0.3% false positives | 0.45: 99.1%, 0.2% |
| loose threshold | 0.28: 91.0% of echoes, 3.9% false positives | 0.24: 94.7%, 5.9% |
| speed (corpus, one thread) | ~0.33 ms per pair | ~3.2 ms per pair |

False-positive rates at equal recall (each configuration's threshold set
to miss the same share of loose echoes):

| configuration | missing 5.3% | missing 8.5% |
|---|---|---|
| semi-global, shipped (zero point 300, shorter-line normalization) | 7.8% | 4.0% |
| semi-global, zero point 300 + mean normalization | 6.5% | 3.6% |
| semi-global, search optimum | 6.0% | 3.2% |
| Waterman–Eggert, zero point 300 + mean normalization | 8.1% | 4.2% |
| Waterman–Eggert, shipped (search optimum) | 5.8% | 3.4% |

With matched settings semi-global is slightly better; tuned, the two are
tied, and semi-global has far fewer false positives when nearly every echo
must be caught (missing 3%: 9.3% vs 12.5%). Averaging or taking the max or
min of the two didn't help. So semi-global is the default and
Waterman–Eggert is an alternative mode. The shipped semi-global gives up
some false positives by keeping shorter-line normalization (see Open
questions). It catches about 35 loose echoes
semi-global misses, mostly fragments of an otherwise different line and
reordered phrases, at the cost of more chance fragments in unrelated
lines.

**Jump was removed.** It was never better than the other two on real
poems. Its strength, reordering, is mostly covered by Waterman–Eggert, and
its reuse of material inflated scores above 1 unless penalized.

Fixture behavior under the shipped settings is pinned by
`fixture_pairs_pass_at_the_shipped_thresholds` (`src/echo/scheme.rs`).
Semi-global's one known gap is three-block permutations (≈0.36–0.40), which
pass as loose but not as strict; Waterman–Eggert scores them 0.50–0.85.

## What the errors are (semi-global, loose)

Misses (91 of 1,011 loose echoes at 0.28; counts below are from an
analysis at 0.276, 86 misses):

- 42: the refrain slot keeps only its end word. By the form's slots these
  are echoes; to the ear they're closer to rhyme.
- 15: one shared word elsewhere, often carried from the end of one line to
  the start of the next.
- 11: a partial phrase inside a longer line.
- 10: nothing shared in the sounds (by meaning, or spelled out).
- 8: shared words, reordered — what Waterman–Eggert is for.

False positives (773 of 19,376 at 0.276): 46% come from one long pantoum
built on anaphora, whose formula lines really do sound alike. The rest are
real echoes the template can't label (a line belongs to one group only),
very short lines, and shared phrasing.

## Open questions

- **Normalization.** Semi-global ships with shorter-line normalization,
  so a truncated repeat scores 1. Mean normalization gives about 15% fewer
  false positives (table above), but a truncation then scores ≈0.77, an
  insertion ≈0.62 instead of ≈0.94, and the thresholds drop (best: strict
  ≈0.39, loose ≈0.22).
- **End-word refrains.** Relabel them, or accept a refrain slot on
  "echo, or the same end word" through the rhyme scorer.
- **Labels the template can't express** (a line echoing two groups), and
  whether one long poem should weigh as much as it does in a pooled AUC.

## Reproducing

```sh
# the report, with pass rates at the shipped thresholds
cargo run --release --example echo_compare -- \
  --corpus data/echo/public --corpus data/echo/private \
  --forms pantoum,duplex,villanelle
# the search (~3 min for semi-global, ~30 for Waterman–Eggert, 12 threads)
cargo run --release --example echo_compare -- --search 120 \
  --corpus data/echo/public --corpus data/echo/private \
  --forms pantoum,duplex,villanelle
```

Results without the private corpus will differ: public poems are almost
all strict.
