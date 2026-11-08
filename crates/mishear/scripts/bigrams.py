"""Writes data/bigrams-en.tsv: word-pair counts from contemporary film and TV
subtitles (OpenSubtitles v2024, English, via OPUS), for the search's bigram
model. Subtitles are the nearest freely available thing to everyday spoken
English, which is what a mishearing should sound like.

    python3 -I crates/mishear/scripts/bigrams.py [--tokens N] [--min-count K]

Streams the start of the corpus (9.5 GB, 8.5 billion tokens; its documents
are not in date or title order, so a prefix is a sample) until N tokens are
counted, 200 million by default (about 300 MB of download). Needs
data/wordfreq-en.tsv first: only pairs of its words are kept.

Lines are `left<TAB>right<TAB>count` for pairs seen at least K times (3 by
default), plus `left<TAB>*<TAB>total`: how often each word is followed by
any word, rarer pairs and unlisted words included. Pairs don't span
sentence punctuation; bracketed sound cues ("[laughing]") and speaker
labels are dropped.

Every 100th line is held out. On those, the script reports the cross-entropy
(and on --eval FILE, any other text) of the model the search uses,
    P(w | v) = (count(v, w) + mu * P(w)) / (count(v) + mu),
with P(w) from wordfreq, for a range of smoothing strengths mu, for pruning
at several K, and (with --compare FILE) for another bigram file, so sources
can be compared on the same pairs. Lower is better; the unigram model is the
baseline.

OPUS distributes the subtitles without owning them (see opus.nlpl.eu). The
counts stay local, in the gitignored data directory.
"""

import argparse
import gzip
import math
import pathlib
import re
import sys
import time
import urllib.request

SOURCE = "https://object.pouta.csc.fi/OPUS-OpenSubtitles/v2024/mono/en.txt.gz"
DATA = pathlib.Path(__file__).resolve().parent.parent / "data"

WORD = re.compile(r"[a-z]+(?:'[a-z]+)*")
# Sound cues, speaker names and markup: [laughing], (sighs), {\an8}, <i>.
CUES = re.compile(r"\[[^\]]*\]|\([^)]*\)|\{[^}]*\}|<[^>]*>")
SPEAKER = re.compile(r"^\s*-?\s*[A-Z][A-Z0-9 .'-]*:\s")
BREAK = re.compile(r"[.!?;:\"()\[\]…♪♫]|--|—|–")

parser = argparse.ArgumentParser()
parser.add_argument("--tokens", type=int, default=200_000_000)
parser.add_argument("--min-count", type=int, default=3)
parser.add_argument("--source", default=SOURCE, help="URL or path of a .txt or .txt.gz")
parser.add_argument("--compare", type=pathlib.Path, help="another bigrams TSV to score")
parser.add_argument("--eval", type=pathlib.Path, help="a text file to score on too")
parser.add_argument("--out", type=pathlib.Path, default=DATA / "bigrams-en.tsv")
args = parser.parse_args()

# The vocabulary, and P(w) as the search computes it: 10^(zipf - 9).
vocab: dict[str, int] = {}
unigram: list[float] = []
for line in (DATA / "wordfreq-en.tsv").read_text().splitlines():
    word, zipf = line.split("\t")
    if word not in vocab:
        vocab[word] = len(vocab)
        unigram.append(10 ** (float(zipf) - 9))
words = list(vocab)


def lines():
    if args.source.startswith("http"):
        response = urllib.request.urlopen(args.source)
        stream = gzip.GzipFile(fileobj=response)
    elif args.source.endswith(".gz"):
        stream = gzip.open(args.source)
    else:
        stream = open(args.source, "rb")
    with stream:
        for raw in stream:
            yield raw.decode("utf-8", errors="replace")


def chunks(line):
    """The line's runs of words between sentence punctuation, as ids (-1:
    not in the vocabulary)."""
    line = SPEAKER.sub("", CUES.sub(" ", line)).replace("’", "'").replace("‘", "'").lower()
    for chunk in BREAK.split(line):
        tokens = WORD.findall(chunk)
        if len(tokens) > 1:
            yield [vocab.get(t, -1) for t in tokens]


pairs: dict[int, int] = {}
totals = [0] * len(vocab)
held_out: list[tuple[int, int]] = []
tokens = 0
start = time.time()
for n, line in enumerate(lines()):
    for ids in chunks(line):
        if n % 100 == 99:
            held_out.extend((v, w) for v, w in zip(ids, ids[1:]) if v >= 0 and w >= 0)
            continue
        tokens += len(ids)
        for v, w in zip(ids, ids[1:]):
            if v >= 0:
                totals[v] += 1
                if w >= 0:
                    key = v << 20 | w
                    pairs[key] = pairs.get(key, 0) + 1
    if n % 1_000_000 == 0 and n:
        rate = tokens / (time.time() - start)
        print(f"  {n:,} lines, {tokens:,} tokens, {len(pairs):,} pairs ({rate:,.0f} tokens/s)", file=sys.stderr)
    if tokens >= args.tokens:
        break
print(f"{n + 1:,} lines, {tokens:,} tokens counted, {len(held_out):,} held-out pairs", file=sys.stderr)
test_sets = {"held-out subtitles": held_out}
if args.eval:
    test_sets[args.eval.name] = [
        (v, w)
        for line in args.eval.read_text().splitlines()
        for ids in chunks(line)
        for v, w in zip(ids, ids[1:])
        if v >= 0 and w >= 0
    ]

# ---- held-out cross-entropy ----

MUS = [10, 30, 100, 300, 1000, 3000]
KS = (1, 2, 3, 5, 10)


def cross_entropy(test, count_of, total_of, mu):
    """Mean -ln P(w | v) over the test pairs (nats)."""
    s = 0.0
    for v, w in test:
        p = (count_of(v, w) + mu * unigram[w]) / (total_of(v) + mu)
        s -= math.log(p)
    return s / len(test)


def ours(min_count):
    def count_of(v, w):
        c = pairs.get(v << 20 | w, 0)
        return c if c >= min_count else 0

    return count_of, totals.__getitem__


other = None
if args.compare:
    other_pairs: dict[int, float] = {}
    other_totals = [0.0] * len(vocab)
    for line in args.compare.read_text().splitlines():
        left, right, count = line.split("\t")
        v = vocab.get(left)
        if v is None:
            continue
        if right == "*":
            other_totals[v] = float(count)
        elif (w := vocab.get(right)) is not None:
            other_pairs[v << 20 | w] = float(count)
    other = (lambda v, w: other_pairs.get(v << 20 | w, 0.0), other_totals.__getitem__)

best_mu = None
for name, test in test_sets.items():
    uni = -sum(math.log(unigram[w]) for _, w in test) / len(test)
    print(f"\n{name}: {len(test):,} pairs; cross-entropy, nats per word (unigram {uni:.3f})")
    header = "  ".join(f"K={k:<5}" for k in KS)
    print(f"{'mu':>6}  {header}" + (f"  {args.compare.name}" if other else ""))
    for mu in MUS:
        row = [cross_entropy(test, *ours(k), mu) for k in KS]
        line = f"{mu:>6}  " + "  ".join(f"{x:<7.3f}" for x in row)
        if other:
            line += f"  {cross_entropy(test, *other, mu):.3f}"
        print(line)
    if best_mu is None:
        best_mu = min(MUS, key=lambda mu: cross_entropy(test, *ours(args.min_count), mu))

# ---- write ----

args.out.parent.mkdir(parents=True, exist_ok=True)
kept = 0
with args.out.open("w") as f:
    for v, total in enumerate(totals):
        if total:
            f.write(f"{words[v]}\t*\t{total}\n")
    for key, count in sorted(pairs.items()):
        if count >= args.min_count:
            f.write(f"{words[key >> 20]}\t{words[key & 0xFFFFF]}\t{count}\n")
            kept += 1
size = args.out.stat().st_size / 1e6
print(f"\n{kept:,} pairs seen {args.min_count}+ times -> {args.out} ({size:.0f} MB); best mu at K={args.min_count}: {best_mu}")
