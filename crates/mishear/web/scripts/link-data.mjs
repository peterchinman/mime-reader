// Links the engine's data into public/data so the dev server and the build
// serve it: CMUdict and the ENABLE word list (committed), and the generated
// word-frequency list and bigram counts (crates/mishear/data, gitignored).
// Missing optional files are skipped.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.resolve(here, '../public/data');
fs.mkdirSync(out, { recursive: true });

const files = [
  ['cmudict-0.7b', '../../../../data/CMUdict/cmudict-0.7b', true],
  ['wordfreq-en.tsv', '../../data/wordfreq-en.tsv', true],
  ['bigrams-en.tsv', '../../data/bigrams-en.tsv', false],
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
  fs.rmSync(to, { force: true });
  fs.symlinkSync(from, to);
}
console.log(`linked data into ${out}`);
