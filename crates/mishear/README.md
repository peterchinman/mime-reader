# mishear

A mishearing engine built on mime-reader. It has three parts:

- **Fuzzy phonetic search**: the word sequences that best match a phoneme
  sequence ([`src/search.rs`](src/search.rs)).
- **Deformations**: rules and vowel-space knobs that make a line sound like
  an accent ([`src/deform.rs`](src/deform.rs), [`src/knobs.rs`](src/knobs.rs),
  [`src/accents.rs`](src/accents.rs)).
- **The mishearing engine**: it deforms a line and hears it back, preferring
  words the line doesn't contain ([`src/engine.rs`](src/engine.rs)).

```
$ cargo run --release -p mishear -- hear "kiss the sky"
kiss the sky
   2.03  phonetic 0.80  shared 0  kisses guy
   2.07  phonetic 0.33  shared 0  case thus guy
   2.08  phonetic 0.99  shared 0  cases guy
   2.09  phonetic 0.68  shared 0  case this gay
   2.10  phonetic 0.70  shared 0  kisses chi
   2.12  phonetic 0.48  shared 0  case this guy
```

## Why it's a separate crate

It uses nothing but mime-reader's public API: CMUdict, the vowel and
consonant distance engines, and `Line`. It adds:

- data (a word-frequency list and bigram counts);
- policy (which words count as common, how much fluency weighs against
  sound);
- caricature accents.

None of that belongs in a poetry-analysis library's API. The search and the
deformation primitives could be promoted into the library later, if poems
need them, for example to suggest near-rhymes or score accent-aware echoes.

## Setup

The generated data is not committed. It lives in `crates/mishear/data/`,
which is gitignored.

```sh
# Word frequencies (wordfreq, data CC BY-SA 4.0)
python3 -m venv /tmp/wfenv && /tmp/wfenv/bin/pip install wordfreq
/tmp/wfenv/bin/python -I crates/mishear/scripts/wordfreq_list.py 100000

# Word-pair counts for the bigram model (without them, words are scored
# alone): streams Google Books' English Fiction 2-grams (~30 GB, not stored,
# ~16 min), keeps pairs of lexicon words, and packs them into
# data/bigrams-en.bin (45 MB; 29 MB gzipped). Needs ~1 GB of temporary space.
crates/mishear/scripts/google_bigrams.sh
```

The ENABLE word list ([`data/ENABLE`](../../data/ENABLE), public domain,
committed) keeps names, brands and abbreviations out of the lexicon, as in
rhyme-golf. A word must be in it, or be a contraction or possessive of a
word in it ("it's", "don't", "people's"; not "america's" or "o'brien").
ENABLE lists every inflected form, so "heard", "women" and "became" are in.
It lacks days and months ("june", "monday") and recent words ("email",
"website", "online"). `--all-words` turns the filter off.

## Commands

```sh
cargo run --release -p mishear -- help
cargo run --release -p mishear -- search K IH1 S DH AH0 S K AY1 -n 5
cargo run --release -p mishear -- hear "the light is gone from the hill"
cargo run --release -p mishear -- deform "the light is gone from the hill" --accent irish
cargo run --release -p mishear -- mishear "the light is gone from the hill" --accent cockney
cargo run --release -p mishear -- mishear "…" --vowels mirror:front-back --knob th-stopping
cargo run --release -p mishear -- examples --accent new-zealand --lines 6 --public
cargo run --release -p mishear -- sweep "kiss the sky" --lm-values 0,0.05,0.2 --penalty-values 0,0.3
cargo run --release -p mishear -- cost      # timing table
cargo run --release -p mishear -- list      # accents and knobs
```

Deformation options combine and apply in order:

- `--accent NAME` adds a preset.
- `--knob NAME` adds a consonant knob.
- `--vowels SPEC` adds a vowel move.
- `--intensity F` scales every step's rate, from 0 to 1.

## Web app

`web/` is a page that exposes every knob. The engine is compiled to
WebAssembly and runs in a Web Worker. It needs `wasm-pack` and the data from
Setup.

```sh
cd crates/mishear/web
npm install
npm run dev     # builds the wasm, links the data into public/data, starts Vite
npm run build   # the same, into dist/
```

What the page has:

- **Input:** a line of text, or ARPAbet phonemes heard as given (no
  deformation, no novelty).
- **Random line:** picks from the public-domain poems in `data/echo/public`.
  The private corpus is never bundled.
- **Deformation:** off by default. A switch applies the steps or bypasses
  them, so you can compare without deleting anything; adding a step switches
  it on. The steps form an ordered list (accent presets, consonant knobs,
  vowel moves with masks and Labov's tendencies), plus intensity and seed.
  For a pull step, you can click the chart to choose its target.
- **Vowel chart:** a preview of the vowel-space moves.
- **Results:** the hearings, with their cost breakdown; a grid aligning each
  heard word with the phonemes it covers; and, in a collapsible "Sounds"
  section, each word's spoken and deformed phonemes, with the deformation's
  edits marked.
- **Search, cost and lexicon panels:** every field of `SearchConfig`,
  `CostConfig` and the lexicon options. Defaults come from the Rust side.
  Changing the lexicon rebuilds it in the worker.
- **Sweep:** the CLI `sweep` for any numeric search or cost knob, or for
  intensity or seed.
- **Saving:** settings are kept in localStorage, and "Copy link" puts them in
  the URL.

`npm run data` symlinks CMUdict, ENABLE and the generated word lists into
`public/data`, which is gitignored, and gzips the packed word-pair counts
there; the page unzips them as they arrive. A build
copies them into `dist/`. That copy includes the wordfreq list, which is CC
BY-SA 4.0, so a hosted build has to carry that license's attribution. It
also includes the Google Books word-pair counts (`bigrams-en.bin.gz`, CC BY
3.0; see "Bigrams"), which need crediting too.

## Search

A match covers the input with words, each word aligned to a contiguous span
of the input. Its cost adds up three things:

- **Phonetic cost.** A weighted edit distance whose substitution costs come
  from mime-reader's vowel and consonant distance engines, on a 0–1 scale.
- **Language-model cost.** `0.005 × −ln P(word | previous word)` plus `0.4`
  per word, from word-pair counts in Google Books fiction,
  Dirichlet-smoothed toward wordfreq's frequencies (see "Bigrams" below).
  With `--unigram`, `P(word)` from wordfreq alone.
- **Extra costs.** The mishearing engine charges 0.9 for each word the
  original line contains, and a fifth of that for function words; 0.2 for
  each word sharing a stem with the line's (`--root`); 0.2 for each of the
  line's word boundaries a hearing keeps (`--crossing`). And each hearing
  after the first pays 1.0 for each word an earlier one used (`--variety`;
  see "Variety").

There are two passes:

1. For each start position, a depth-first walk of the pronunciation trie
   carries an edit-distance column over the input. That scores every word
   against every span starting there. The walk prunes subtrees that can't
   finish a word within the bound.
2. A k-best dynamic program runs over the resulting word lattice.

### The prior's knobs

The weights below are judgment calls, picked by looking at output. They
weren't fitted to data, so they are all flags. `sweep` loads the engine once
and runs a grid of them. The defaults were set on 2026-11-08 to settings
chosen in the web page: few results, a light word prior, a heavier word
penalty, bigrams, and some pull toward new roots and boundaries.

The examples and timings in the rest of "Search" were measured under the
earlier defaults. To reproduce them, add `--unigram --lm 0.05
--word-penalty 0.1 --novelty 1 --root 0 --crossing 0 -n 8`, unless an
example names its own.

| flag | default | what it trades |
|---|---|---|
| `--lm` | 0.005 | sound against word frequency |
| `--word-penalty` | 0.4 | many short words against few long ones |
| `--pronunciations first\|all` | all | more ways to hear a word, against more chance matches |
| `--bigram-prior` | 1000 | with bigrams: trusting word pairs (small) against unigram frequency (large) |
| `--unigram` | off (bigrams on) | exactness and speed, against fluency |
| `--beam` | 100 | with bigrams: closer to exact (wider) against speed |
| `--vocab`, `--all-words` | 50k, filtered | coverage against obscure words, names and brands |
| `--novelty`, `--function-share` | 0.9, 0.2 | departing from the original's words |
| `--root` | 0.2 | departing from the original's roots too ("flying" for "fly in") |
| `--crossing` | 0.2 | hearings that cut across the original's word boundaries |
| `--variety` | 1.0 | each hearing differing from those above it, against the plain top N |
| `--stress-mismatch` | 0 | keeping the line's rhythm (extra cost when a stressed vowel pairs with an unstressed one) |
| `--swap` | off | adjacent consonants exchanged as one edit ("ask" ↔ "aks") |
| `--all-spellings` | off | listing homophone spellings separately, rather than merging hearings that sound the same |

What the sweep showed (`hear "kiss the sky"`, and two public-domain lines:
"Sleep rounds with equal zest" in `new-zealand`, "Cried on his shoulder but"
in `cockney`), with the unigram model:

- **`--lm 0`: sound alone.** Near-exact, obscure words: "cuss thus chi",
  "slip round swathe equals iced". The word penalty changes nothing at
  this setting.
- **`--lm` 0.02–0.1: the useful band.**
  - At 0.05: "case this guy", "slip round's with equals east", "grow don is
    should a by".
  - At 0.1: commoner words, further from the sound: "case this to", "slip
    round the sequel just", "written is should a by". A word penalty of 0.3
    here gives fewer, longer words: "rebounds with equalized".
- **`--lm` 0.2 and up: frequency alone.** "cases to", "philosophical just".
- **`--pronunciations first`** changed nothing on these lines.
- **`--bigram-prior`** (with `--bigrams`): at 30, "cretin is shut up a"
  comes first; from 300, "crew don is shut up a". See "Bigrams" below for
  how the default was chosen.

### Bigrams

By default the search scores each word by the one before it (`--unigram`
turns this off), with word-pair counts from Google Books' English Fiction
2-grams (2020): 21.6M pairs of lexicon words, summed over all years, from
about 200 million 2-gram lines. The model is
`P(w | v) = (count(v, w) + (μ + r(v))·P(w)) / (count(v) + μ)`, smoothed
toward wordfreq's `P(w)` with μ = 100,000 (the counts run to billions).
`r(v)` is the part of `v`'s count no listed pair accounts for (pairs pruned
or left out, words outside the lexicon), which goes to the backoff instead
of vanishing.

**Why these counts.** `examples/bigram_eval.rs` scores a counts file by
cross-entropy (nats per word, lower is better) on word pairs it never
counted: every 10th line from the first 1M lines of OpenSubtitles, and the
WikiText-103 test split. Restricted to pairs of lexicon words, the only
ones a hearing can contain:

| counts | pairs | subtitles | Wikipedia |
|---|---|---|---|
| unigram | – | 7.02 | 7.41 |
| subtitles, 200M tokens (the previous default) | 2.3M | 5.29* | 6.93 |
| Google Books fiction, 2.5M most informative pairs | 2.5M | 5.71 | 6.36 |
| Google Books fiction, 10M | 10M | 5.66 | 6.24 |
| **Google Books fiction, all** | **21.6M** | **5.64** | **6.20** |

\*Flattered: that file was counted from the lines this test set uses.

- **On general English, Google wins by 0.7 nats.** The subtitle counts keep
  their edge on subtitle dialogue. An earlier comparison, before Google:
  subtitle counts beat 1.2M words of Gutenberg books (Austen, Milton,
  Shakespeare) at every μ.
- **More subtitles don't close the gap.** `scripts/bigrams.py
  --checkpoints` counted up to 1.6B subtitle tokens: each doubling gained
  about 0.1 nats on Wikipedia and less on subtitles, where 13% of the test
  lines recur verbatim in what was counted.
- **Giving pruned mass to the backoff matters.** Without `r(v)`, a pair cut
  from the table looked less likely than its word alone: the top 2.5M
  Google pairs scored 7.35 on Wikipedia rather than 6.92 (on all test
  pairs), and the best μ ran off the scale.
- **Pruning by information beats pruning by count**, slightly: a pair's
  information is `c·ln(1 + c/(μ·P(w)))`, how much its count beats backing
  off.
- **Where the gain shows.** At the default `--lm 0.005` the model adds only
  about 0.01–0.03 per word, so bigrams change about a third of the hearings
  and rarely the best; the hearings' word pairs are mostly unseen ("toothy
  rub"), as they should be. At `--lm 0.05`, two thirds change, toward
  pairs the counts know: "velodrome watchful of people", "societies was
  never scene", "case this guy".

**Packed.** As text the counts are 514 MB, and compress only to 134–167 MB.
`examples/pack_bigrams.rs` keeps the pairs of lexicon words and writes each
left word's pairs as gaps between right-word ids (varints) and each count as
one byte, `round(8·ln c)`, within 6%, which the scores don't notice. That is
45 MB, 29 MB gzipped (25.6 MB with brotli). The engine holds it as one array
per field (about 5 bytes a pair) and finds a pair by binary search in its
left word's row: the command line starts in 0.4 s at 226 MB, the web page
has its first hearings in 1.5 s, with 208 MB of memory. A lexicon is a view
of the table, so rebuilding one doesn't reread it.

The Google Books Ngram data is CC BY 3.0: a hosted build must credit it.
`scripts/bigrams.py` still builds subtitle counts
(`data/bigrams-subtitles.tsv`), and `pack_bigrams` makes them the default
instead.

### Evaluating against known mondegreens

`eval` takes the mishearings of public-domain texts in
[`mondegreens.tsv`](mondegreens.tsv) ("gladly the cross I'd bear" →
"gladly the cross eyed bear", "bringing in the sheaves" → "bringing in the
sheep", …). For each grid point, it reports where the famous mishearing
ranks among the top 50 hearings of the original.

```sh
cargo run --release -p mishear -- eval --all-words --all-spellings \
  --lm-values 0.02,0.05 --novelty-values 0,0.1 --stress-values 0,0.3 \
  --variety 0 --unigram --word-penalty 0.1 --root 0 --crossing 0
```

At its best (`--lm 0.02`, `--novelty 0`), only 2 of the 12 reachable
mishearings make the top 50: "hark the harold angels sing" at rank 1 and
"gladly the cross eyed bear" at rank 2 (MRR 0.125). With `--bigrams`, both
are at rank 1 (MRR 0.167), and both stay in the top 10 at `--novelty 0.1`,
where the unigram model keeps only "harold" there (rank 6).

These ranks are among the plain top 50 (`--variety 0`). With the default
`--variety 1`, "harold" stays first, but "gladly the cross eyed bear" drops
out of the top 50 and "jose can you see" comes in at 19–23 (MRR 0.087 at
`--lm 0.02`). A mondegreen that changes one word looks, to variety, like
the hearings above it.

The others lose to closer neighbours. "Bringing in the thieves" (sound
cost 0.30) beats "bringing in the sheep" (0.94), and "while shepherds
washed their socks" needs two changes at once. Famous mondegreens are
famous for their meaning, not for being the nearest sound. So this set
can't tune the weights toward anything but "stay close to the original",
which pulls against `--novelty`. Stress and swaps barely change it.

Tuning on taste needs labels: hearings someone has marked good or bad.

Many classic mondegreens differ from the original only in spelling
(herald → harold, I'd → eyed). Merging hearings by sound, the default,
keeps one spelling per sound: the cheapest, after novelty. So with novelty
on, "hark the harold angels sing" can come out as "…angels singh". Use
`--all-spellings` to see every spelling.

### Roots and boundaries

- **`--root F` charges F for each heard word that shares a stem with a word
  of the line.** Stems come from Snowball's English stemmer (Porter2):
  "fly", "flies" and "flying" all give "fli". The same word counts too, so
  `--root` adds to `--novelty` for it. `--function-share` applies when the
  line's word is a function word. Irregular forms ("flew") aren't caught.
  - In "the swallows fly in", the plain top hearings (`--variety 0`) all
    end in "flying" at 0 ("thus willows flying"). From `--root 1` they end
    in "frying".
  - In "Since Persia fell at Marathon", "marathons" gets past `--novelty`,
    which only catches the identical word. `--root` catches it.
- **`--crossing F` rewards each of the line's word boundaries that a
  hearing cuts across** ("this guy" for "the sky" cuts "the|sky"). A
  negative cost would break the search's pruning, so it's charged instead
  as F for each boundary a hearing keeps: a word starting exactly there.
  Every hearing keeps or cuts each boundary, so the ranking is the same as
  for a reward, and the search stays exact. The costs shown include it.
  - In "Since Persia fell at Marathon", the plain top four (`--variety 0`)
    all keep "at|marathon" at 0 ("…affair at marathons"). From 0.3 they all
    cut across it ("sin spurs of era telethon").
  - On that line it costs no time: about 100 ms at every setting.
- `mishear` prints each hearing's counts: `shared` (same word), `root`
  (another word with the same stem), `kept` (boundaries kept, out of the
  line's) and `reused` (words an earlier hearing used).

### Variety

The plain top N tend to be one hearing respelled: "sin spurs affair at
marathons", "seen spurs affair at marathons", "sense persevere at
marathons", … `--variety F` (default 1.0) treats the hearings above each
one as novelty treats the line: each word an earlier hearing used costs F
more, as does any homophone of it ("cents" after "sense"). Function words
pay `--function-share` of that, and with `--root`, words sharing their
stems (and those words' homophones) pay too. Each hearing is then the best one given those above it:

- "Since Persia fell at Marathon": "sin spurs affair at marathons", "sense
  persevere admire of on", "sins per the fella telethon", "cents purchase
  era telephone", "sensors a ferret marathon", "seen spur shuffle add marry
  than", "sisters authoritarian", "signs burgess arid mare often".
- "Kiss the sky": "case this guy", "kisses chi", "guess thus to", "cases
  gay", "gives ask i", "guys the stay", "keys vista", "give just a".

`--variety 0` lists the plain top N, from one search.

**Exact, without a search per hearing.** Penalties only add cost, so one
pass-1 lattice serves every hearing, as long as it keeps per span the N
best words plus one for each penalized word. Pass 2 returns a pool of 24
more than the hearings already taken. The next hearings come from that
pool, rescored with their new penalties, while the best of them is
provably the best overall: it costs no more than the pool's last member
did, and everything outside the pool cost at least that before the new
penalties. A test checks the result against a full search per hearing.

**Cost.** Over 12 public lines with the current defaults, 128 ms per line
on average, against 62 ms for the plain list (2×); with `--unigram`, 79 ms
against 53 ms (1.5×).

### Rhythm and swaps

- **`--stress-mismatch 0.3` keeps the meter.** "Such a maid as was never
  seen!" goes from "sesame does was never scene" to "such amid as was never
  scene". It's slightly faster too: tighter costs prune more. Going to 0.6
  changes nothing more. "Swaying to and fro on his rickety stool" keeps
  its meter without it ("sway into and throw on his rickets tool").
- **`--swap` changes nothing** on the corpus lines tried or in the eval, and
  costs a little more time. Metathesis isn't how lines get misheard.

### Repeated work across bound rounds

Raising the bound until N matches fit spends about 0.8× the last round's
time on the earlier rounds. I tried two ways to avoid that, and neither
pays:

- **Seeding the bound** from a small vocabulary: its N-th best is looser
  than the full vocabulary's, so the one exact round costs more. That saves
  10–13% at best.
- **Faster bound growth** (×2 or ×3, or a higher start): it overshoots, and
  is 25–50% slower.

The remaining ~45% needs a walk that resumes from the previous round's
pruned frontier. That means keeping each frontier node's column, on the
order of 100 MB per line, which isn't worth it at 0.1–0.5 s per line.

**Exactness.** With the unigram model (`--unigram`), the search is exact. It
returns every match within the bound with its exact cost, and a test checks
it against brute force. The argument:

- Costs are non-negative, so a match within the bound has every word within
  it too.
- Keeping each span's N best words loses nothing from the top N.

`best_matches` raises the bound geometrically until it holds N matches, which
gives the exact top N.

With bigrams (the default), the best word for a span depends on the word
before it. Pruning still uses an admissible lower bound, so it is safe. But
the per-span shortlist (30 words) and pass 2's beam (`--beam`, 100 partial
hearings per position) make the result approximate. At the default
`--lm 0.005` a beam of 100 finds the same hearings as one of 400 on 30
public lines (but for one tie broken the other way); at `--lm 0.1` it
makes 3 of 120 hearings slightly worse, and a beam of 50 makes 7.

**Cost.** Corpus lines of 10–33 phonemes (mean 24), on one core, unigram
model:

| vocabulary | exact top 1 | exact top 10 | 10 within 4 | 10 within 6 |
|---|---|---|---|---|
| 20k words | 82 ms | 94 ms | 41 ms | 56 ms |
| 50k words (default) | 192 ms | 220 ms | 96 ms | 145 ms |
| 66k words (all with pronunciations) | 249 ms | 292 ms | 132 ms | 194 ms |

Finding the exact best is barely more expensive than the top 10. Both cost
about one bounded trie walk per input position, which grows linearly with
the vocabulary and the line's length. The bounded search ("N within X") is
about 1.5× cheaper, because it skips the bound-raising rounds. An
approximate search isn't needed. The penalties on shared words, the line's
and earlier hearings', push the bound out. Accents that lengthen the line
take longer.

**Speed.** With the current defaults (the Google counts), mishearings over
12 public lines take 167 ms on average, and 83 ms with `--unigram`; the
web page about 250 ms (500 ms at `--lm 0.05`). With the previous, smaller
subtitle counts they took 130 ms, and 721 ms before the changes below,
which leave the results as they were:

- **The bound jumps to where the hearings are.** The search starts with a
  bound of 2 and raises it until enough hearings fit, rebuilding pass 1
  each round. After each round, the cheapest complete hearing through the
  lattice (a shortest path, by pass 1's costs) tells where the bound needs
  to go: past it by 30%, rather than 1.5× at a time. If it already lies
  beyond the bound, pass 2 is skipped, since it can't find anything. Lines
  took 5.6 rounds on average, now about 2.
- **Pass 1 skips what the line's start rules out.** Every hearing with a
  word starting at position `s` has already paid the cheapest way to cover
  the input before `s`, which the walks from earlier positions have found.
  The walk from `s` gets only what the bound leaves after that, and
  positions nothing cheap enough reaches aren't walked at all.
- **Pass 2 looks each word pair up once.** Partial hearings ending at a
  position often share their last word, so each word's cost after it is
  looked up once per edge, in a pair table with a fast hash (FxHash).
  Partials are kept cheapest first, so an edge stops once even its cheapest
  context can't fit the bound.
- **The bigram beam narrowed from 400 to 100** (see "Exactness"), the only
  change here that can alter a result.

## Deformations

A deformation is a list of steps, each applied to every word:

- **Rules.** Context-sensitive rewrites of a phoneme class: replace, delete,
  insert, voice or devoice. A rule can depend on:
  - the neighbours (vowel, consonant, word edge, "not followed by a vowel");
  - stress;
  - the word's spelling, for features no phoneme context predicts. For
    example, the stage-Irish "tay" for *tea* applies to IY spelled ‹ea›.
- **Simultaneous rule sets.** These are for chain shifts. The Northern Cities
  Shift is a loop (AE → EH → AH → AO → AA → AE), so applying the rules one
  after another would cascade.
- **Vowel moves.** Geometric operations on a vowel chart, after which each
  vowel snaps back to the nearest ARPAbet vowel, keeping its stress.
  Diphthongs move their start and end separately. The chart comes from
  Hillenbrand et al. (1995) formants, Lobanov-normalized (see
  [docs/accent-research.md](docs/accent-research.md), section 2).
  - The moves are `pull:X,Y,AMOUNT` (toward a point), `shift`, `onset`
    (diphthong starts only), `rotate:DEGREES`, `scale`,
    `mirror:front-back` and `mirror:height`.
  - Any move can be masked with `@tense`, `@lax`, `@front`, `@back` or
    `@diphthongs`.
  - Labov's chain-shift tendencies are named knobs: `tense-up`, `lax-down`,
    `onsets-down`, `back-to-front`.
- **Word-level steps.** For example, French final stress.

Each step has a rate (0–1): the share of matching sites it applies to. A
seeded hash picks the sites, so the same input always deforms the same way.

### Consonant knobs

- `th-stopping`, `th-fronting`
- `non-rhotic`, `h-dropping`
- `final-devoicing`, `l-vocalization`, `g-dropping`, `t-glottaling`
- `cluster-reduction`, `yod-dropping`, `epenthesis`, `lenition`
- swaps: `w-to-v`, `v-to-w`, `v-to-b`, `r-to-l`, `l-to-r`

### Accent presets

- Irish: `irish` (stage), `irish-rural`
- British: `scottish`, `cockney`, `rp`, `posh`
- North American: `southern`, `northern-cities`
- Other Englishes: `new-zealand`, `australian`, `jamaican`
- Non-native: `german`, `french`, `spanish`, `japanese`
- Historical: `great-vowel-shift`, `reverse-vowel-shift`

Each rule cites a rule id in
[docs/accent-research.md](docs/accent-research.md). That file gives, per
rule, the varieties it belongs to, its sources and how lossy ARPAbet makes
it. It also marks the rules it couldn't verify.

These are **caricatures by construction**: what a General American ear hears,
squeezed through 39 phonemes. Features ARPAbet can't express are left out:
the Irish slit-t, monophthongal FACE and GOAT, fronted GOOSE, and intonation.
Lexical features are approximations: the BATH words, the Irish and Scottish
splits of the *bird* vowel, and "tay".

## Examples

These use public-domain lines from `data/echo/public` (`examples --public
--line-seed 7`). Each shows the top match, with the defaults and with
`--unigram`.

| line | deformation | heard | heard, `--unigram` |
|---|---|---|---|
| Thought gets dry in the brain. | irish | taught gates troy into blain | taught gates troy into blain |
| Sleep rounds with equal zest | new-zealand | sri browned swathe equals east | sri browned swathe equals east |
| Cried on his shoulder but | cockney | crow donna's shout abba | crow donna's shout abba |
| Don' know's I'd mind his goin' | southern | done no sod monde hiss going | done no sod monde hiss going |
| Such a maid as was never seen! | reverse-vowel-shift | saw sonata swathe nether sane | saw sonata swathe nether sane |
| Such a maid as was never seen! | rotate:-60 | sesame de sway sniffer soon | sesame days yeah sniffer soon |
| Sleep rounds with equal zest | mirror:front-back | slew blinds without will just | slew blinds without will just |
| Sleep rounds with equal zest | spanish | asleep round sway sequels asked | asleep round sway sequels asked |
| Sly, musical, a jewel in rhyme, | japanese | sue lima huge acreage ooh uh ruin lima | sue lima huge acreage ooh uh ruin lima |
| Sly, musical, a jewel in rhyme, | pull:1,1,0.5 | slim use ecology align room | slim use ecology align room |

## Limitations and next steps

- **Fluency.** Scoring words alone (`--unigram`) makes hearings word salad
  (or poetry). Bigrams, the default, add word-pair counts from Google Books,
  which help locally ("guess this guy") but can't see past the previous
  word ("the tomatoes was never sign"), and at the default `--lm` they
  weigh little. A longer-range model, such
  as a trigram model or a small neural rescorer over the top 50, would go
  further. The backoff could also try Kneser–Ney's continuation counts
  rather than wordfreq's frequencies.
- **The ENABLE filter has gaps.** It drops days, months and recent words
  ("june", "email", "online"), and admits a few names that are also
  ordinary words ("john", "jones", "texas"). A short allow-list would cover
  the first.
- **Snapping chooses one vowel.** The research suggests emitting the
  near-ties as weighted alternatives, which is exactly where mishearings come
  from.
- **The rotate knob is rigid.** Chain shifts are better modelled as rotation
  along the edge of the vowel space (research, section 3).
- **CMUdict's vowel classes limit some rules.** AA covers LOT, PALM and
  START; THOUGHT is split inconsistently; there's no yod after coronals.
  Accent rules aimed at one lexical set hit the others too (research,
  section 0).
