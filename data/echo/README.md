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

Come back to me, Sweet! ...
<line 2>
<line 3>

<stanza 2 ...>
```

`form`, `title` and `author` are required. One blank line ends the header.
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

## First comparison run (defaults, before any decision)

`cargo run --release --example echo_compare` with the default config
(aligner choice and threshold still open). 12,494 pairs scored; 4,666
corpus pairs were dropped for words CMUdict lacks (*aweary*, *faery*,
*villanelle*, *st*, *vexation*, *gwine*, *ev'ry*, …).

| set | aligner | AUC | best threshold | balanced acc. | margin |
|---|---|---|---|---|---|
| fixture (36 / 8) | semi_global | 0.972 | 0.558 | 0.931 | −0.152 |
| | waterman_eggert | 1.000 | 0.567 | 1.000 | 0.078 |
| | jump | 1.000 | 0.707 | 1.000 | 0.215 |
| corpus (463 / 11,641) | semi_global | 0.998 | 0.613 | 0.981 | −0.699 |
| | waterman_eggert | 0.998 | 0.613 | 0.980 | −0.628 |
| | jump | 0.993 | 0.735 | 0.973 | −0.956 |

Per poem (precision@k / MRR / perfectly separated): semi_global 0.962 /
0.990 / 44 of 52; waterman_eggert 0.963 / 0.990 / 44; jump 0.915 / 0.963 /
42. Every villanelle and both pantoums rank perfectly under semi_global and
waterman_eggert.

Observations to weigh before choosing:

- **Jump reuse inflates scores.** With `reuse_penalty = 0`, consuming the
  longer line lets jump re-use the shorter one instead of gapping, so
  insertions and extensions score above 1 (fixture class 3 mean 1.23) and
  non-echoes score higher (corpus non-echo mean ≈ 0.46 vs ≈ 0.27).
  `ECHO_CONFIG='{"reuse_penalty":500}'` removes the inflation and gives jump
  corpus AUC 0.996, fixture margin 0.282, per-poem precision@k 0.949.
- **Moves.** semi_global can't see re-ordering (two-block swaps ≈ 0.64,
  three-block permutations ≈ 0.43); waterman_eggert recovers most of it
  (0.93 / 0.77); jump all of it (0.97 / 0.95). Greedy local runs can
  overextend across a block junction when the neighbors happen to sound
  alike, which costs waterman_eggert on some reversals.
- **Shorter-line normalization** scores a short line contained in a longer
  one as a full echo (some blues non-echo pairs reach 1.0).
- Fixture → corpus threshold transfer: Δ −0.055 (semi_global), −0.047
  (waterman_eggert), −0.028 (jump).
