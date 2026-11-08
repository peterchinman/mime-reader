#!/bin/sh
# Builds data/bigrams-en.bin, the word-pair counts the search uses by
# default, from the Google Books Ngram English Fiction 2-grams (2020, CC BY
# 3.0): streams its 47 files (about 30 GB, never stored; about 16 minutes at
# 40 MB/s) through examples/google_bigrams.rs, which keeps lowercased pairs
# of the frequency list's words summed over the years, then packs the pairs
# whose words can be in the lexicon (examples/pack_bigrams.rs). Needs about
# 1 GB of temporary space. Extra arguments go to the packer (--keep N).
set -eu
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
index=http://storage.googleapis.com/books/ngrams/books/20200217/eng-fiction/eng-fiction-2-ngrams_exports.html
curl -sS --fail "$index" | grep -oE 'http[^"]+/2-[0-9]+-of-[0-9]+\.gz' > "$tmp/urls"
echo "$(wc -l < "$tmp/urls") files"
cargo build --release -p mishear --example google_bigrams --example pack_bigrams
for u in $(cat "$tmp/urls"); do
  # A file cut short would leave its words undercounted: note it, and don't pack.
  { curl -sS --fail --retry 5 "$u" | gunzip -c; } || touch "$tmp/failed"
  echo "$(date +%T) $(basename "$u")" >&2
done | ../../target/release/examples/google_bigrams --out "$tmp/google"
if [ -e "$tmp/failed" ]; then
  echo "a download failed: not packing (run again)" >&2
  exit 1
fi
../../target/release/examples/pack_bigrams "$tmp/google-all.tsv" data/bigrams-en.bin "$@"
