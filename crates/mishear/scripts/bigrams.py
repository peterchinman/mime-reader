"""Writes data/bigrams-subtitles.tsv: word-pair counts from contemporary film
and TV subtitles (OpenSubtitles v2024, English, via OPUS), an alternative to
the default Google Books counts (scripts/google_bigrams.sh), and the harness
that compared them. To search with them, pack them over the default:

    cargo run --release -p mishear --example pack_bigrams -- \
      crates/mishear/data/bigrams-subtitles.tsv crates/mishear/data/bigrams-en.bin

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

The first 1M lines are never counted: every 10th of them is the test set.
On it (and on --eval FILE, any other text) the script reports the
cross-entropy of the model the search uses,
    P(w | v) = (count(v, w) + mu * P(w)) / (count(v) + mu),
with P(w) from wordfreq, for a range of smoothing strengths mu, for pruning
at several K, and (with --compare FILE) for another bigram file, so sources
can be compared on the same pairs. Lower is better; the unigram model is the
baseline. --checkpoints 25e6,50e6,… reports along the way: a learning curve.
It also reports how many test lines recur verbatim in what was counted
(subtitle sites carry several versions of a film), which flatters the
subtitle test set but not --eval.

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
parser.add_argument("--out", type=pathlib.Path, default=DATA / "bigrams-subtitles.tsv")
parser.add_argument("--held-out-lines", type=int, default=1_000_000)
parser.add_argument("--checkpoints", default="", help="token counts to report at, e.g. 25e6,50e6")
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


def normalized(line):
    return " ".join(WORD.findall(CUES.sub(" ", line).lower()))


eval_pairs = []
if args.eval:
    eval_pairs = [
        (v, w)
        for line in args.eval.read_text().splitlines()
        for ids in chunks(line)
        for v, w in zip(ids, ids[1:])
        if v >= 0 and w >= 0
    ]

pairs: dict[int, int] = {}
totals = [0] * len(vocab)
held_out: list[tuple[int, int]] = []
test_lines: set[int] = set()  # hashes of test lines of 6+ words
recurring: set[int] = set()
checkpoints = sorted(int(float(c)) for c in args.checkpoints.split(",") if c)
curve = []


def evaluate(test, ks, mus):
    """Cross-entropy (nats per word) at each (min count, mu)."""
    counts = [pairs.get(v << 20 | w, 0) for v, w in test]
    context = [totals[v] for v, _ in test]
    prior = [unigram[w] for _, w in test]
    out = {}
    for k in ks:
        kept = [c if c >= k else 0 for c in counts]
        for mu in mus:
            s = 0.0
            for c, n, p in zip(kept, context, prior):
                s -= math.log((c + mu * p) / (n + mu))
            out[k, mu] = s / len(test)
    return out


def checkpoint():
    ks, mus = (1, 3, 5, 10), (300, 1000, 3000, 10000)
    sizes = {k: 0 for k in ks}
    for c in pairs.values():
        for k in ks:
            if c >= k:
                sizes[k] += 1
    row = {"tokens": tokens, "sizes": sizes, "subs": evaluate(held_out, ks, mus)}
    if eval_pairs:
        row["eval"] = evaluate(eval_pairs, ks, mus)
    curve.append(row)
    best = lambda r: min(r.items(), key=lambda kv: kv[1])
    line = f"  @ {tokens / 1e6:,.0f}M tokens: pairs {sizes[1]:,} (3+: {sizes[3]:,})"
    for name in ("subs", "eval"):
        if name in row:
            (k, mu), x = best({km: v for km, v in row[name].items() if km[0] == 3})
            line += f"; {name} {x:.3f} (K=3, mu {mu})"
    print(line, file=sys.stderr, flush=True)


tokens = 0
start = time.time()
for n, line in enumerate(lines()):
    if n < args.held_out_lines:
        if n % 10 == 0:
            for ids in chunks(line):
                held_out.extend((v, w) for v, w in zip(ids, ids[1:]) if v >= 0 and w >= 0)
            text = normalized(line)
            if text.count(" ") >= 5:
                test_lines.add(hash(text))
        continue
    if test_lines:
        h = hash(normalized(line))
        if h in test_lines:
            recurring.add(h)
    for ids in chunks(line):
        tokens += len(ids)
        for v, w in zip(ids, ids[1:]):
            if v >= 0:
                totals[v] += 1
                if w >= 0:
                    key = v << 20 | w
                    pairs[key] = pairs.get(key, 0) + 1
    if n % 1_000_000 == 0:
        rate = tokens / (time.time() - start)
        print(f"  {n:,} lines, {tokens:,} tokens, {len(pairs):,} pairs ({rate:,.0f} tokens/s)", file=sys.stderr, flush=True)
    if checkpoints and tokens >= checkpoints[0]:
        checkpoints.pop(0)
        checkpoint()
    if tokens >= args.tokens:
        break
print(f"{n + 1:,} lines, {tokens:,} tokens counted, {len(held_out):,} test pairs", file=sys.stderr)
print(f"{len(recurring):,} of {len(test_lines):,} test lines of 6+ words recur verbatim in the counted text ({len(recurring) / max(1, len(test_lines)):.1%})")
test_sets = {"held-out subtitles": held_out}
if args.eval:
    test_sets[args.eval.name] = eval_pairs

if curve:
    print("\nlearning curve: cross-entropy at the best mu, for pairs seen K+ times")
    print(f"{'tokens':>8}  {'pairs 3+':>10}  " + "  ".join(f"{name} K={k}" for name in ("subs", "eval") if name in curve[0] for k in (1, 3, 10)))
    for row in curve:
        cells = []
        for name in ("subs", "eval"):
            if name not in row:
                continue
            for k in (1, 3, 10):
                cells.append(f"{min(x for (kk, _), x in row[name].items() if kk == k):>10.3f}")
        print(f"{row['tokens'] / 1e6:>7,.0f}M  {row['sizes'][3]:>10,}  " + "  ".join(cells))

# ---- held-out cross-entropy ----

MUS = [10, 30, 100, 300, 1000, 3000, 10000, 30000]
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
