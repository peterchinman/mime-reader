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

# Bigram counts from NLTK's packaging of public-domain Project Gutenberg texts
curl -sSLo /tmp/gutenberg.zip \
  https://raw.githubusercontent.com/nltk/nltk_data/gh-pages/packages/corpora/gutenberg.zip
python3 -I crates/mishear/scripts/bigrams.py /tmp/gutenberg.zip
```

`/usr/share/dict/words` (Webster's 2nd, present on macOS) is used when it
exists. It keeps names and brands out of the lexicon: a word must appear
there in lowercase, or its base form must once a regular ending is stripped.

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

## Search

A match covers the input with words, each word aligned to a contiguous span
of the input. Its cost adds up three things:

- **Phonetic cost.** A weighted edit distance whose substitution costs come
  from mime-reader's vowel and consonant distance engines, on a 0–1 scale.
- **Language-model cost.** `0.05 × −ln P(word | previous word)` plus `0.1`
  per word. The bigram model is Dirichlet-smoothed toward wordfreq's
  unigram frequencies.
- **Extra costs.** The mishearing engine charges 1.0 for each word the
  original line contains, and a fifth of that for function words.

There are two passes:

1. For each start position, a depth-first walk of the pronunciation trie
   carries an edit-distance column over the input. That scores every word
   against every span starting there. The walk prunes subtrees that can't
   finish a word within the bound.
2. A k-best dynamic program runs over the resulting word lattice.

### The prior's knobs

The weights below are judgment calls, picked by looking at output. They
weren't fitted to data, so they are all flags. `sweep` loads the engine once
and runs a grid of them.

| flag | default | what it trades |
|---|---|---|
| `--lm` | 0.05 | sound against word frequency |
| `--word-penalty` | 0.1 | many short words against few long ones |
| `--pronunciations first\|all` | all | more ways to hear a word, against more chance matches |
| `--bigram-prior` | 100 | trusting word pairs (small) against unigram frequency (large) |
| `--unigram` | off | exactness and speed, against fluency |
| `--vocab`, `--all-words` | 50k, filtered | coverage against obscure words, names and brands |
| `--novelty`, `--function-share` | 1.0, 0.2 | departing from the original's words |
| `--stress-mismatch` | 0 | keeping the line's rhythm (extra cost when a stressed vowel pairs with an unstressed one) |
| `--swap` | off | adjacent consonants exchanged as one edit ("ask" ↔ "aks") |
| `--all-spellings` | off | listing homophone spellings separately, rather than merging hearings that sound the same |

What the sweep showed (`hear "kiss the sky"`, and two public-domain lines):

- **`--lm 0`: sound alone.** Near-exact, obscure words: "kish thus chi",
  "slip round's withe equals ast". The word penalty changes nothing at this
  setting.
- **`--lm` 0.02–0.1: the useful band.**
  - At 0.05: "case thus guy", "case this guy", "crow don is so to be".
  - At 0.1: more fluent, but further from the sound: "slip round the sequel
    just", "written is so to be".
- **`--lm` 0.2 and up: frequency alone.** "of the", "the ground the sequel
  just". Add a word penalty of 0.3 and the line collapses into one common
  long word: "inevitable", "scientifically".
- **`--pronunciations first`** changes a few results ("crow donna's soda by"
  rather than "crow don is so to be"), with no clear winner.
- **`--bigram-prior`.** At 10 the model trusts sparse pairs ("cretin is so to
  be"); at 1000 it acts like the unigram model.

### Evaluating against known mondegreens

`eval` takes the mishearings of public-domain texts in
[`mondegreens.tsv`](mondegreens.tsv) ("gladly the cross I'd bear" →
"gladly the cross eyed bear", "bringing in the sheaves" → "bringing in the
sheep", …). For each grid point, it reports where the famous mishearing
ranks among the top 50 hearings of the original.

```sh
cargo run --release -p mishear -- eval --all-words --all-spellings \
  --lm-values 0.02,0.05 --novelty-values 0,0.1 --stress-values 0,0.3
```

At its best (`--lm 0.02`, `--novelty 0`), only 2 of the 12 reachable
mishearings make the top 50: "hark the harold angels sing" at rank 1 and
"gladly the cross eyed bear" at rank 2.

The others lose to closer neighbours. "Bringing in the thieves" (sound
cost 0.30) beats "bringing in the sheep" (0.94), and "while shepherds
washed their socks" needs two changes at once. Famous mondegreens are
famous for their meaning, not for being the nearest sound. So this set
can't tune the weights toward anything but "stay close to the original",
which pulls against `--novelty`; any novelty at all drops both hits. Stress
and swaps don't change it.

Tuning on taste needs labels: hearings someone has marked good or bad.

Many classic mondegreens differ from the original only in spelling
(herald → harold, I'd → eyed). Merging hearings by sound, the default,
keeps one spelling per sound: the cheapest, after novelty. So with novelty
on, "hark the harold angels sing" can come out as "…angels singh". Use
`--all-spellings` to see every spelling.

### Rhythm and swaps

- **`--stress-mismatch 0.3` keeps the meter.** "Swaying to and fro on his
  rickety stool" goes from "…on his wicked east all" to "…on his rickets
  tool", which is also closer in sound. "Such a maid as was never seen!"
  goes from "sesame does was never scene" to "such amid as was never
  scene". It's slightly faster too: tighter costs prune more. Going to 0.6
  changes nothing more on these lines.
- **`--swap` changes nothing** on the corpus lines tried or in the eval, and
  costs about 5% more time. Metathesis isn't how lines get misheard.

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

With bigrams, the best word for a span depends on the word before it. Pruning
still uses an admissible lower bound, so it is safe. But the per-span
shortlist (30 words) makes the result approximate.

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
approximate search isn't needed. Mishearings, with bigrams, typically take
0.1–0.25 s per line, because the penalty on shared words pushes the bound
out. Accents that lengthen the line take longer; Japanese epenthesis, for
example, takes about 1 s.

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
--line-seed 7`). Each shows the top match.

| line | deformation | heard |
|---|---|---|
| Thought gets dry in the brain. | irish | taught gates troy and a plain |
| Sleep rounds with equal zest | new-zealand | slip round the sequel just |
| Cried on his shoulder but | cockney | crow don is so to be |
| Don' know's I'd mind his goin' | southern | done no judgment he's going |
| Such a maid as was never seen! | reverse-vowel-shift | such an odd as was never sane |
| Such a maid as was never seen! | rotate:-60 | sesame days was neither soon |
| Sleep rounds with equal zest | mirror:front-back | sloop rinds without will just |
| Sleep rounds with equal zest | spanish | asleep round swathe equals asked |
| Sly, musical, a jewel in rhyme, | japanese | sure i muse ecology ruin limo |
| Sly, musical, a jewel in rhyme, | pull:1,1,0.5 | slow music ritual own room |

## Limitations and next steps

- **Fluency is the weak point.** The bigram model comes from 1.3M words of
  Gutenberg prose and verse, and is too small to rule out word salad. A
  larger n-gram model, or a small neural rescorer over the top 50, would
  help most.
- **Names still leak through the common-word filter** ("sri", "elaine").
- **Snapping chooses one vowel.** The research suggests emitting the
  near-ties as weighted alternatives, which is exactly where mishearings come
  from.
- **The rotate knob is rigid.** Chain shifts are better modelled as rotation
  along the edge of the vowel space (research, section 3).
- **CMUdict's vowel classes limit some rules.** AA covers LOT, PALM and
  START; THOUGHT is split inconsistently; there's no yod after coronals.
  Accent rules aimed at one lexical set hit the others too (research,
  section 0).
