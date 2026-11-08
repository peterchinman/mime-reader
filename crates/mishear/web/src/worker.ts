// Hosts the engine off the main thread: a search can take a second, and
// rebuilding the lexicon a few.
import init, { MishearApp, catalog, defaults, type LexiconOptions } from 'mishear';
import { sameLexicon, type FromWorker, type ToWorker } from './protocol';

const post = (message: FromWorker) => self.postMessage(message);

let app: MishearApp | null = null;
let lexicon: LexiconOptions | null = null;
let data = '';
let bigrams: 'unloaded' | 'loaded' | 'missing' = 'unloaded';
let memory: WebAssembly.Memory | null = null;

/** Word-pair counts are big: fetched when first needed, packed and gzipped
 * (else as text). Without them the engine scores words alone. */
async function ensureBigrams(): Promise<void> {
  if (bigrams !== 'unloaded' || !app) return;
  post({ type: 'status', message: 'Fetching word-pair counts (once)…' });
  const bytes = (await fetchGzipped(data + 'bigrams-en.bin.gz')) ?? (await fetchBytes(data + 'bigrams-en.tsv'));
  if (bytes === undefined) {
    bigrams = 'missing';
    return;
  }
  post({ type: 'status', message: 'Loading word-pair counts…' });
  app.loadBigrams(bytes);
  bigrams = 'loaded';
}

async function fetchBytes(url: string): Promise<Uint8Array | undefined> {
  const res = await fetch(url);
  return res.ok ? new Uint8Array(await res.arrayBuffer()) : undefined;
}

/** A gzipped file, unzipped as it arrives (unless the server already did,
 * by Content-Encoding). */
async function fetchGzipped(url: string): Promise<Uint8Array | undefined> {
  const res = await fetch(url);
  if (!res.ok || !res.body) return undefined;
  const [peek, body] = res.body.tee();
  const first = await peek.getReader().read();
  const gzipped = first.value !== undefined && first.value[0] === 0x1f && first.value[1] === 0x8b;
  const stream = gzipped ? body.pipeThrough(new DecompressionStream('gzip')) : body;
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function fetchText(url: string, required: boolean): Promise<string | undefined> {
  const res = await fetch(url);
  if (res.ok) return res.text();
  if (required) throw new Error(`${url}: HTTP ${res.status} (run \`npm run data\`)`);
  return undefined;
}

async function start(overrides: Partial<LexiconOptions>): Promise<void> {
  const t0 = performance.now();
  post({ type: 'status', message: 'Loading the engine…' });
  memory = (await init()).memory;
  const options: LexiconOptions = { ...defaults().lexicon, ...overrides };
  data = import.meta.env.BASE_URL + 'data/';
  post({ type: 'status', message: 'Fetching CMUdict and word frequencies…' });
  const [dict, frequencies, words] = await Promise.all([
    fetch(data + 'cmudict-0.7b').then((r) => {
      if (!r.ok) throw new Error(`${data}cmudict-0.7b: HTTP ${r.status} (run \`npm run data\`)`);
      return r.arrayBuffer();
    }),
    fetchText(data + 'wordfreq-en.tsv', true),
    fetchText(data + 'enable1.txt', false),
  ]);
  post({ type: 'status', message: 'Building the lexicon…' });
  app = new MishearApp(new Uint8Array(dict), frequencies ?? '', undefined, words, options);
  lexicon = options;
  post({
    type: 'ready',
    catalog: catalog(),
    defaults: defaults(),
    info: app.lexiconInfo(),
    lexicon: options,
    loadMs: performance.now() - t0,
  });
}

self.onmessage = async (event: MessageEvent<ToWorker>) => {
  const message = event.data;
  try {
    if (message.type === 'init') {
      await start(message.lexicon);
      return;
    }
    if (!app) throw new Error('the engine is not loaded yet');
    if (message.type === 'run') {
      if (message.request.search.bigrams) await ensureBigrams();
      let rebuildMs = 0;
      if (!sameLexicon(message.lexicon, lexicon)) {
        const t = performance.now();
        app.setLexicon(message.lexicon);
        lexicon = message.lexicon;
        rebuildMs = performance.now() - t;
      }
      const t = performance.now();
      const result = app.run(message.request);
      post({
        type: 'result',
        id: message.id,
        result,
        ms: performance.now() - t,
        info: app.lexiconInfo(),
        rebuildMs,
        memoryMb: memory ? memory.buffer.byteLength / 1e6 : 0,
        notice:
          message.request.search.bigrams && bigrams === 'missing'
            ? 'No word-pair counts (run crates/mishear/scripts/bigrams.py, then npm run data): scoring words alone.'
            : undefined,
      });
    } else if (message.type === 'preview') {
      post({ type: 'preview', id: message.id, preview: app.preview(message.deformation) });
    }
  } catch (err) {
    post({
      type: 'error',
      id: 'id' in message ? message.id : undefined,
      message: String(err instanceof Error ? err.message : err),
    });
  }
};
