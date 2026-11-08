"""Writes data/bigrams-en.tsv: word-pair counts from NLTK's packaging of
Project Gutenberg texts (public domain), for the search's bigram model.

    curl -sSLo /tmp/gutenberg.zip \\
      https://raw.githubusercontent.com/nltk/nltk_data/gh-pages/packages/corpora/gutenberg.zip
    python3 -I scripts/bigrams.py /tmp/gutenberg.zip

Lines are `left<TAB>right<TAB>count` for pairs seen at least twice, plus
`left<TAB>*<TAB>total` for every left word: how often it is followed by
any word, singletons included. Pairs don't span sentence punctuation. The
King James Bible is left out: a third of the words, all of them archaic.
"""

import collections
import pathlib
import re
import sys
import zipfile

WORD = re.compile(r"[a-z]+(?:'[a-z]+)?")
BREAK = re.compile(r"[.!?;:()\"\[\]]")

archive = pathlib.Path(sys.argv[1])
pairs = collections.Counter()
totals = collections.Counter()
words = 0
with zipfile.ZipFile(archive) as z:
    for name in sorted(z.namelist()):
        if not name.endswith(".txt") or "bible" in name:
            continue
        text = z.read(name).decode("latin-1").lower().replace("’", "'")
        for chunk in BREAK.split(text):
            tokens = WORD.findall(chunk)
            words += len(tokens)
            for left, right in zip(tokens, tokens[1:]):
                pairs[(left, right)] += 1
                totals[left] += 1

out = pathlib.Path(__file__).resolve().parent.parent / "data" / "bigrams-en.tsv"
with out.open("w") as f:
    for left, total in sorted(totals.items()):
        f.write(f"{left}\t*\t{total}\n")
    kept = 0
    for (left, right), count in sorted(pairs.items()):
        if count >= 2:
            f.write(f"{left}\t{right}\t{count}\n")
            kept += 1
print(f"{words} words; {kept} pairs seen twice or more; {len(totals)} left words -> {out}")
