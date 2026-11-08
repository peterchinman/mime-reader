// Hosts the engine off the main thread: a search can take a second, and
// rebuilding the lexicon a few.
import init, { MishearApp, catalog, defaults, type LexiconOptions } from 'mishear';
import { sameLexicon, type FromWorker, type ToWorker } from './protocol';

const post = (message: FromWorker) => self.postMessage(message);

let app: MishearApp | null = null;
let lexicon: LexiconOptions | null = null;
let data = '';
let bigramsLoaded = false;

/** Word-pair counts are big and off by default: fetched on first use. */
async function ensureBigrams(): Promise<void> {
  if (bigramsLoaded || !app) return;
  post({ type: 'status', message: 'Fetching word-pair counts…' });
  const text = await fetchText(data + 'bigrams-en.tsv', false);
  if (text === undefined) {
    throw new Error('No word-pair counts at data/bigrams-en.tsv: run crates/mishear/scripts/bigrams.py, then `npm run data`.');
  }
  post({ type: 'status', message: 'Loading word-pair counts…' });
  app.loadBigrams(text);
  bigramsLoaded = true;
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
  await init();
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
