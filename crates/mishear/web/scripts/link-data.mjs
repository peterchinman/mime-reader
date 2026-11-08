// Links the engine's data into public/data so the dev server and the build
// serve it: CMUdict and the ENABLE word list (committed), and the generated
// word-frequency list and word-pair counts (crates/mishear/data,
// gitignored). The packed counts are gzipped for the page, which unzips them
// as they arrive. Missing optional files are skipped.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.resolve(here, '../public/data');
fs.mkdirSync(out, { recursive: true });

const files = [
  ['cmudict-0.7b', '../../../../data/CMUdict/cmudict-0.7b', true],
  ['wordfreq-en.tsv', '../../data/wordfreq-en.tsv', true],
  ['bigrams-en.tsv', '../../data/bigrams-en.tsv', false],
  ['bigrams-en.bin.gz', '../../data/bigrams-en.bin', false],
  ['enable1.txt', '../../../../data/ENABLE/enable1.txt', true],
];
for (const [name, source, required] of files) {
  const from = path.isAbsolute(source) ? source : path.resolve(here, source);
  const to = path.join(out, name);
  if (!fs.existsSync(from)) {
    if (required) {
      console.error(`missing ${from}: see crates/mishear/README.md, "Setup"`);
      process.exit(1);
    }
    console.warn(`skipping ${name}: ${from} not found`);
    continue;
  }
  if (name.endsWith('.gz')) {
    // Compressed once, and again only when the source changes.
    if (!fs.existsSync(to) || fs.statSync(to).mtimeMs < fs.statSync(from).mtimeMs) {
      console.log(`gzipping ${from}…`);
      fs.rmSync(to, { force: true });
      fs.writeFileSync(to, zlib.gzipSync(fs.readFileSync(from), { level: 9 }));
    }
    continue;
  }
  fs.rmSync(to, { force: true });
  fs.symlinkSync(from, to);
}
console.log(`linked data into ${out}`);
