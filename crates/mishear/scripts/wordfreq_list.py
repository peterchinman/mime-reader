"""Writes data/wordfreq-en.tsv: the most frequent English words with their
Zipf frequency (log10 of occurrences per billion words), from the wordfreq
package (https://github.com/rspeer/wordfreq; data CC BY-SA 4.0, so the
generated file is not committed).

    python3 -m venv /tmp/wfenv && /tmp/wfenv/bin/pip install wordfreq
    /tmp/wfenv/bin/python scripts/wordfreq_list.py [N]
"""

import pathlib
import sys

import wordfreq

n = int(sys.argv[1]) if len(sys.argv) > 1 else 100_000
out = pathlib.Path(__file__).resolve().parent.parent / "data" / "wordfreq-en.tsv"
out.parent.mkdir(exist_ok=True)
with out.open("w") as f:
    for word in wordfreq.top_n_list("en", n):
        f.write(f"{word}\t{wordfreq.zipf_frequency(word, 'en'):.2f}\n")
print(f"wrote {n} words to {out}")
