import './style.css';
import type {
  CatalogDto,
  CostDto,
  DefaultsDto,
  DeformationDto,
  LexiconInfoDto,
  LexiconOptions,
  Mode,
  PreviewDto,
  RequestDto,
  ResultDto,
  SearchDto,
  StepDto,
} from 'mishear';
import { LINES } from './lines';
import { sameLexicon as same, type FromWorker, type ToWorker } from './protocol';

// ---------------------------------------------------------------- knobs ----

interface NumberKnob {
  type: 'number';
  key: string;
  label: string;
  hint: string;
  min: number;
  max: number;
  step: number;
  /** Optional knobs (`null` in Rust) get an on/off box; this is the value they turn on at. */
  onValue?: number;
  /** Rebuilds the lexicon: apply on release, not while dragging. */
  heavy?: boolean;
}
interface BoolKnob {
  type: 'bool';
  key: string;
  label: string;
  hint: string;
  heavy?: boolean;
}
type Knob = NumberKnob | BoolKnob;
type Group = 'search' | 'cost' | 'lexicon' | 'deformation';

const SEARCH_KNOBS: Knob[] = [
  { type: 'number', key: 'results', label: 'results', hint: 'How many hearings to return: the exact top N.', min: 1, max: 50, step: 1 },
  {
    type: 'number',
    key: 'within',
    label: 'only within cost',
    hint: 'Return hearings costing at most this (up to N of them), exactly. Faster; may return fewer. Off: the bound starts at 2 and rises until N are found.',
    min: 0.5,
    max: 15,
    step: 0.1,
    onValue: 4,
  },
  {
    type: 'number',
    key: 'lm_weight',
    label: 'word-prior weight',
    hint: 'Cost per nat of a word’s surprisal, −ln P(word). 0: every word is as likely as any other (rarer words, odder hearings); higher: common words win.',
    min: 0,
    max: 0.5,
    step: 0.005,
  },
  { type: 'number', key: 'word_penalty', label: 'word penalty', hint: 'Cost per word. Higher prefers fewer, longer words.', min: 0, max: 2, step: 0.01 },
  {
    type: 'bool',
    key: 'bigrams',
    label: 'bigrams',
    hint: 'Score each word given the one before, from word-pair counts in contemporary film and TV subtitles. More fluent, but approximate (exact only up to the shortlist and 400 partial hearings per position) and slower. Off: each word alone (unigram), and exact.',
  },
  { type: 'number', key: 'shortlist', label: 'bigram shortlist', hint: 'Words kept per input span when bigrams are on.', min: 5, max: 200, step: 1 },
  {
    type: 'bool',
    key: 'distinct_sounds',
    label: 'merge same-sound hearings',
    hint: 'Treat hearings that sound alike (“there” / “their”) as one, keeping the cheapest spelling. Off: list every spelling.',
  },
  {
    type: 'number',
    key: 'novelty',
    label: 'novelty',
    hint: 'Extra cost per heard word that the line already contains. 0 lets the line hear itself back.',
    min: 0,
    max: 5,
    step: 0.05,
  },
  {
    type: 'number',
    key: 'root',
    label: 'same-root penalty',
    hint: 'Extra cost per heard word sharing a stem with a word of the line: “flying” for “fly in”, “flies” for “fly” (Snowball English stemmer). The same word counts too, on top of novelty. Irregular forms (“flew”) aren’t caught.',
    min: 0,
    max: 5,
    step: 0.05,
  },
  {
    type: 'number',
    key: 'function_share',
    label: 'function-word share',
    hint: 'The share of the novelty and same-root costs charged when the line’s word is a function word (the 150 most frequent).',
    min: 0,
    max: 1,
    step: 0.05,
  },
  {
    type: 'number',
    key: 'crossing',
    label: 'cross word boundaries',
    hint: 'Reward per word boundary of the line that a hearing cuts across (“this guy” for “the sky” cuts the|sky). Charged instead as a cost on each boundary a hearing keeps: that ranks hearings the same way and keeps the search exact, so the costs shown include it.',
    min: 0,
    max: 3,
    step: 0.05,
  },
  { type: 'number', key: 'max_bound', label: 'max bound', hint: 'How far the top-N search may raise its bound before it settles for fewer.', min: 2, max: 60, step: 0.5 },
];

const COST_KNOBS: Knob[] = [
  { type: 'number', key: 'vowel_substitution', label: 'vowel substitution', hint: 'Cost of hearing one vowel as the farthest-apart other; nearer vowels cost proportionally less.', min: 0, max: 3, step: 0.05 },
  { type: 'number', key: 'consonant_substitution', label: 'consonant substitution', hint: 'The same for consonants.', min: 0, max: 3, step: 0.05 },
  { type: 'number', key: 'stressed_vowel_indel', label: 'stressed vowel add/drop', hint: 'Hearing a stressed vowel (a syllable) that isn’t there, or missing one.', min: 0, max: 3, step: 0.05 },
  { type: 'number', key: 'unstressed_vowel_indel', label: 'unstressed vowel add/drop', hint: 'The same for an unstressed vowel (mostly a schwa).', min: 0, max: 3, step: 0.05 },
  { type: 'number', key: 'consonant_indel', label: 'consonant add/drop', hint: 'Hearing a consonant that isn’t there, or missing one.', min: 0, max: 3, step: 0.05 },
  {
    type: 'number',
    key: 'stress_mismatch',
    label: 'rhythm (stress mismatch)',
    hint: 'Extra for hearing a stressed vowel as an unstressed one or the reverse: how closely a hearing must keep the line’s rhythm.',
    min: 0,
    max: 2,
    step: 0.05,
  },
  {
    type: 'number',
    key: 'swap',
    label: 'adjacent swap',
    hint: 'Hearing two adjacent consonants swapped (“ask” ↔ “aks”) as one edit at this cost. Off: no swaps.',
    min: 0,
    max: 2,
    step: 0.05,
    onValue: 0.5,
  },
];

const LEXICON_KNOBS: Knob[] = [
  { type: 'number', key: 'vocabulary', label: 'vocabulary', hint: 'The most frequent words kept. Changing it rebuilds the lexicon.', min: 1000, max: 100000, step: 1000, heavy: true },
  { type: 'bool', key: 'all_pronunciations', label: 'every pronunciation', hint: 'Every CMUdict pronunciation of a word, or only the first.', heavy: true },
  {
    type: 'bool',
    key: 'common_words',
    label: 'ENABLE words only',
    hint: 'Keep only words in the ENABLE word list (no names, brands or abbreviations; every inflected form listed), plus contractions and possessives of them: “it’s”, “don’t”, “people’s”. The same list rhyme-golf uses.',
    heavy: true,
  },
  {
    type: 'number',
    key: 'bigram_prior',
    label: 'bigram smoothing μ',
    hint: 'Dirichlet smoothing: how far a rarely seen word pair falls back on the single-word frequency.',
    min: 1,
    max: 1000,
    step: 1,
    heavy: true,
  },
];

const DEFORMATION_KNOBS: Knob[] = [
  { type: 'number', key: 'intensity', label: 'intensity', hint: 'Scales every rule’s rate: at 0.5 a rule fires at about half its sites.', min: 0, max: 1, step: 0.05 },
  { type: 'number', key: 'seed', label: 'seed', hint: 'Which sites a partial rate picks.', min: 0, max: 999, step: 1 },
];

const KNOB_EXAMPLES: Record<string, string> = {
  'th-stopping': '“thing” → “ting”, “this” → “dis”',
  'th-fronting': '“think” → “fink”, “brother” → “bruvver”',
  'non-rhotic': '“car” → “cah”, “bird” → “bud”',
  'h-dropping': '“house” → “ouse”',
  'final-devoicing': '“bad” → “bat”, “dogs” → “docks”',
  'l-vocalization': '“milk” → “miuk”, “ball” → “baw”',
  'g-dropping': '“going” → “goin’”',
  't-glottaling': '“butter” → “bu’er”, “got” → “go’”',
  'cluster-reduction': '“west” → “wes”, “hand” → “han”',
  'yod-dropping': '“tune” → “toon”, “new” → “noo”',
  'w-to-v': 'W → V',
  'v-to-w': 'V → W',
  'v-to-b': 'V → B',
  'r-to-l': 'R → L',
  'l-to-r': 'L → R',
  epenthesis: '“film” → “fillum”',
  lenition: 'between vowels B → V, D → DH: “baby” → “bavy”',
};

// --------------------------------------------------------- vowel moves ----

interface Param {
  name: string;
  min: number;
  max: number;
  step: number;
  value: number;
}
interface MoveKind {
  label: string;
  params: Param[];
  /** Takes an `@mask` (the Labov tendencies have their own). */
  masked: boolean;
  spec: (p: number[]) => string;
}
const xy = (name: string, value: number): Param => ({ name, min: -0.5, max: 1.5, step: 0.01, value });
const delta = (name: string, value: number): Param => ({ name, min: -0.6, max: 0.6, step: 0.01, value });
const amount = (value: number): Param => ({ name: 'amount', min: 0, max: 0.6, step: 0.01, value });

const MOVES: Record<string, MoveKind> = {
  pull: {
    label: 'pull toward a point',
    params: [xy('x (front 0 → back 1)', 1), xy('y (low 0 → high 1)', 1), { name: 'amount', min: 0, max: 1, step: 0.01, value: 0.4 }],
    masked: true,
    spec: (p) => `pull:${p[0]},${p[1]},${p[2]}`,
  },
  shift: { label: 'shift', params: [delta('dx (toward back)', 0), delta('dy (toward high)', 0.15)], masked: true, spec: (p) => `shift:${p[0]},${p[1]}` },
  onset: {
    label: 'shift diphthong onsets',
    params: [delta('dx (toward back)', 0), delta('dy (toward high)', -0.15)],
    masked: true,
    spec: (p) => `onset:${p[0]},${p[1]}`,
  },
  rotate: { label: 'rotate about the center', params: [{ name: 'degrees (counterclockwise)', min: -180, max: 180, step: 1, value: -30 }], masked: true, spec: (p) => `rotate:${p[0]}` },
  scale: { label: 'spread / crowd about the center', params: [{ name: 'factor', min: 0, max: 2, step: 0.01, value: 0.6 }], masked: true, spec: (p) => `scale:${p[0]}` },
  'mirror-fb': { label: 'mirror front ↔ back', params: [], masked: true, spec: () => 'mirror:front-back' },
  'mirror-h': { label: 'mirror high ↔ low', params: [], masked: true, spec: () => 'mirror:height' },
  'tense-up': { label: 'tense vowels rise (Labov)', params: [amount(0.15)], masked: false, spec: (p) => `tense-up:${p[0]}` },
  'lax-down': { label: 'lax vowels fall (Labov)', params: [amount(0.15)], masked: false, spec: (p) => `lax-down:${p[0]}` },
  'onsets-down': { label: 'diphthong onsets fall (Labov)', params: [amount(0.15)], masked: false, spec: (p) => `onsets-down:${p[0]}` },
  'back-to-front': { label: 'back vowels move front (Labov)', params: [amount(0.2)], masked: false, spec: (p) => `back-to-front:${p[0]}` },
};
const MASKS = ['', 'tense', 'lax', 'front', 'back', 'diphthongs'] as const;
type Mask = (typeof MASKS)[number];

type Step =
  | { kind: 'accent'; name: string }
  | { kind: 'knob'; name: string }
  | { kind: 'vowels'; move: string; params: number[]; mask: Mask };

function stepDto(step: Step): StepDto {
  if (step.kind !== 'vowels') return { kind: step.kind, value: step.name };
  const move = MOVES[step.move];
  let spec = move.spec(step.params);
  if (move.masked && step.mask) spec += `@${step.mask}`;
  return { kind: 'vowels', value: spec };
}

// ---------------------------------------------------------------- state ----

interface State {
  /** The saved format: 3 marks settings saved as changes from the defaults. */
  v?: number;
  mode: Mode;
  text: string;
  phonemes: string;
  steps: Step[];
  /** Whether the steps apply: off keeps them, unapplied. */
  deform: boolean;
  deformation: { intensity: number; seed: number };
  search: SearchDto;
  cost: CostDto;
  lexicon: LexiconOptions;
  autoRun: boolean;
  sweepKnob: string;
  sweepValues: string;
  soundsOpen: boolean;
}

// Only settings changed from their defaults are saved, so a change of default
// reaches everyone who hadn't touched that setting.
const STORAGE_KEY = 'mishear-web-state-v3';
const OLD_STORAGE_KEY = 'mishear-web-state-v2';
const FIRST_LINE = 'Since Persia fell at Marathon';
let defaults: DefaultsDto;
let catalog: CatalogDto;
let state: State;
let lexiconInfo: LexiconInfoDto | null = null;

function defaultState(): State {
  return {
    mode: 'mishear',
    text: FIRST_LINE,
    phonemes: 'K IH1 S DH AH0 S K AY1',
    steps: [],
    deform: false,
    deformation: { intensity: 1, seed: 0 },
    search: { ...defaults.search },
    cost: { ...defaults.cost },
    lexicon: { ...defaults.lexicon },
    autoRun: true,
    sweepKnob: 'search.lm_weight',
    sweepValues: '0, 0.02, 0.05, 0.1, 0.2',
    soundsOpen: true,
  };
}

/** The settings in the link, or else the saved ones, in the current format. */
function readSaved(): Partial<State> | null {
  try {
    const hash = location.hash.slice(1);
    if (hash) return upgrade(JSON.parse(decodeURIComponent(hash)));
    const current = localStorage.getItem(STORAGE_KEY);
    if (current) return upgrade(JSON.parse(current));
    return upgrade(JSON.parse(localStorage.getItem(OLD_STORAGE_KEY) ?? 'null'));
  } catch {
    return null;
  }
}

/** Earlier formats always applied their steps, and defaulted to an Irish
 * accent: drop that default, and keep anything else switched on. */
function upgrade(saved: Partial<State> | null): Partial<State> | null {
  if (!saved || saved.v === 3) return saved;
  const steps = saved.steps ?? [];
  const oldDefault = steps.length === 1 && steps[0].kind === 'accent' && steps[0].name === 'irish';
  return { ...saved, v: 3, steps: oldDefault ? [] : steps, deform: !oldDefault && steps.length > 0 };
}

function loadState(): State {
  const base = defaultState();
  const saved = readSaved();
  if (!saved) return base;
  // Merge over the defaults, so knobs added since are filled in.
  return {
    ...base,
    ...saved,
    deformation: { ...base.deformation, ...unsaved(saved.deformation) },
    search: { ...base.search, ...unsaved(saved.search) },
    cost: { ...base.cost, ...unsaved(saved.cost) },
    lexicon: { ...base.lexicon, ...unsaved(saved.lexicon) },
    steps: (saved.steps ?? base.steps).filter(validStep),
  };
}

function validStep(step: Step): boolean {
  if (step.kind === 'accent') return catalog.accents.some((a) => a.name === step.name);
  if (step.kind === 'knob') return catalog.knobs.includes(step.name);
  return step.move in MOVES && step.params.length === MOVES[step.move].params.length;
}

/** `state` minus the knobs at their defaults (`null` marks an optional knob turned off). */
function savedState(): Partial<State> {
  const base = defaultState();
  const changed = (values: object, defaults: object) =>
    Object.fromEntries(
      Object.entries({ ...defaults, ...values })
        .filter(([k, v]) => v !== (defaults as Record<string, unknown>)[k])
        .map(([k, v]) => [k, v ?? null]),
    );
  const top = Object.fromEntries(
    Object.entries(state).filter(([k, v]) => JSON.stringify(v) !== JSON.stringify(base[k as keyof State])),
  );
  return {
    ...top,
    v: 3,
    deformation: changed(state.deformation, base.deformation) as State['deformation'],
    search: changed(state.search, base.search) as unknown as SearchDto,
    cost: changed(state.cost, base.cost) as unknown as CostDto,
    lexicon: changed(state.lexicon, base.lexicon) as unknown as LexiconOptions,
  };
}

function saveState(): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(savedState()));
}

/** Undoes `savedState`'s `null`s. */
function unsaved<T extends object>(values: T | undefined): Partial<T> {
  return Object.fromEntries(Object.entries(values ?? {}).map(([k, v]) => [k, v ?? undefined])) as Partial<T>;
}

function groupValues(group: Group): Record<string, unknown> {
  return group === 'deformation' ? state.deformation : (state[group] as unknown as Record<string, unknown>);
}
function groupDefaults(group: Group): Record<string, unknown> {
  if (group === 'deformation') return defaultState().deformation;
  return defaults[group] as unknown as Record<string, unknown>;
}

/** The steps as built, applied or not (for the chart). */
function stepsDto(): DeformationDto {
  return { steps: state.steps.map(stepDto), intensity: state.deformation.intensity, seed: state.deformation.seed };
}

/** What the line is deformed by: nothing while deformation is off. */
function deformationDto(): DeformationDto {
  return state.deform ? stepsDto() : { ...stepsDto(), steps: [] };
}

function request(): RequestDto {
  return {
    mode: state.mode,
    text: state.mode === 'mishear' ? state.text : state.phonemes,
    deformation: deformationDto(),
    search: state.search,
    cost: state.cost,
  };
}

// --------------------------------------------------------------- worker ----

const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
let nextId = 1;
const waiting = new Map<number, (m: FromWorker) => void>();

function call(message: ToWorker & { id: number }): Promise<FromWorker> {
  return new Promise((resolve) => {
    waiting.set(message.id, resolve);
    worker.postMessage(message);
  });
}

worker.onmessage = (event: MessageEvent<FromWorker>) => {
  const m = event.data;
  if (m.type === 'status') return setStatus(m.message, true);
  if (m.type === 'ready') return onReady(m);
  if ('id' in m && m.id !== undefined && waiting.has(m.id)) {
    waiting.get(m.id)!(m);
    waiting.delete(m.id);
  } else if (m.type === 'error') {
    setStatus('');
    showError(m.message);
  }
};
worker.onerror = (e) => showError(`The engine worker failed: ${e.message}`);

// ---------------------------------------------------------------- dom ----

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const el = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { className?: string } = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] => {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
};
const SVG = 'http://www.w3.org/2000/svg';
const svg = (tag: string, attrs: Record<string, string | number>, text?: string): SVGElement => {
  const node = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  if (text !== undefined) node.textContent = text;
  return node;
};

function setStatus(message: string, busy = false): void {
  const s = $('status');
  s.textContent = message;
  s.classList.toggle('busy', busy);
}
function showError(message: string): void {
  const e = $('error');
  e.textContent = message;
  e.hidden = !message;
}

const fmt = (x: number, digits = 2) => x.toFixed(digits);
const thousands = (x: number) => Math.round(x).toLocaleString('en-US');

/** Builds one knob's controls; `onChange(heavy)` fires on every edit. */
function knobControl(group: Group, knob: Knob, onChange: () => void): HTMLElement {
  const values = groupValues(group);
  const def = groupDefaults(group)[knob.key];
  const wrap = el('div', { className: 'knob' });
  const head = el('div', { className: 'knob-head' });
  const reset = el('button', { type: 'button', className: 'reset', textContent: 'reset', title: `Default: ${def ?? 'off'}` });
  const markChanged = () => wrap.classList.toggle('changed', values[knob.key] !== def);
  const commit = () => {
    markChanged();
    onChange();
  };

  if (knob.type === 'bool') {
    const box = el('input', { type: 'checkbox', checked: Boolean(values[knob.key]) });
    box.addEventListener('change', () => {
      values[knob.key] = box.checked;
      commit();
    });
    reset.addEventListener('click', () => {
      values[knob.key] = def;
      box.checked = Boolean(def);
      commit();
    });
    head.append(el('label', { className: 'name' }, box, ` ${knob.label}`), reset);
    wrap.append(head, el('p', { className: 'hint', textContent: knob.hint }));
    wrap.dataset.key = knob.key;
    markChanged();
    return wrap;
  }

  const optional = knob.onValue !== undefined;
  const current = values[knob.key] as number | undefined;
  const shown = current ?? knob.onValue ?? knob.min;
  const range = el('input', { type: 'range', min: String(knob.min), max: String(knob.max), step: String(knob.step), value: String(shown) });
  const number = el('input', { type: 'number', min: String(knob.min), step: String(knob.step), value: String(shown) });
  const box = el('input', { type: 'checkbox', checked: current !== undefined && current !== null, title: 'on / off' });
  const setValue = (v: number | undefined, from?: HTMLInputElement) => {
    values[knob.key] = v;
    if (v !== undefined) {
      if (from !== range) range.value = String(v);
      if (from !== number) number.value = String(v);
    }
    wrap.classList.toggle('off', optional && v === undefined);
  };
  const read = (input: HTMLInputElement) => {
    let v = Number(input.value);
    if (!Number.isFinite(v)) return;
    if (knob.step >= 1) v = Math.round(v);
    if (optional && !box.checked) box.checked = true;
    setValue(v, input);
  };
  // Light knobs follow the slider; heavy ones apply when it is released.
  range.addEventListener('input', () => {
    number.value = range.value;
    if (!knob.heavy) {
      read(range);
      commit();
    }
  });
  range.addEventListener('change', () => {
    read(range);
    commit();
  });
  number.addEventListener('change', () => {
    read(number);
    commit();
  });
  box.addEventListener('change', () => {
    setValue(box.checked ? Number(number.value) : undefined);
    commit();
  });
  reset.addEventListener('click', () => {
    const d = def as number | undefined | null;
    box.checked = d !== undefined && d !== null;
    setValue(d ?? undefined);
    if (d === undefined || d === null) {
      range.value = number.value = String(knob.onValue);
    }
    commit();
  });
  head.append(el('label', { className: 'name' }, ...(optional ? [box, ' '] : []), knob.label), reset);
  wrap.append(head, el('div', { className: 'knob-inputs' }, range, number), el('p', { className: 'hint', textContent: knob.hint }));
  wrap.classList.toggle('off', optional && (current === undefined || current === null));
  wrap.dataset.key = knob.key;
  markChanged();
  return wrap;
}

function renderKnobs(group: Exclude<Group, 'deformation'>, knobs: Knob[], container: HTMLElement): void {
  container.replaceChildren(
    ...knobs.map((k) =>
      knobControl(group, k, () => {
        if (group === 'lexicon' && !sameLexicon()) setStatus('The lexicon will rebuild on the next run.');
        changed();
      }),
    ),
  );
  if (group === 'lexicon' && lexiconInfo && !lexiconInfo.common_words_available) {
    const box = container.querySelector<HTMLInputElement>('[data-key="common_words"] input');
    if (box) {
      box.disabled = true;
      box.title = 'The ENABLE word list wasn’t found (see `npm run data`).';
    }
  }
}

// ---------------------------------------------------------------- steps ----

let pickingStep: number | null = null;

function newStep(choice: string): Step {
  const [kind, name] = choice.split(':', 2) as ['accent' | 'knob' | 'vowels', string];
  if (kind === 'vowels') return { kind, move: name, params: MOVES[name].params.map((p) => p.value), mask: '' };
  return { kind, name };
}

function renderAddStep(): void {
  const add = $<HTMLSelectElement>('add-step');
  const group = (label: string, options: [string, string][]) =>
    el('optgroup', { label }, ...options.map(([value, text]) => el('option', { value, textContent: text })));
  add.replaceChildren(
    el('option', { value: '', textContent: '+ add a step…' }),
    group(
      'Accents',
      catalog.accents.map((a) => [`accent:${a.name}`, a.name]),
    ),
    group(
      'Consonant knobs',
      catalog.knobs.map((k) => [`knob:${k}`, `${k}  (${KNOB_EXAMPLES[k] ?? ''})`]),
    ),
    group(
      'Vowel moves',
      Object.entries(MOVES).map(([k, m]) => [`vowels:${k}`, m.label]),
    ),
  );
  add.onchange = () => {
    if (!add.value) return;
    state.steps.push(newStep(add.value));
    state.deform = true;
    renderDeformToggle();
    add.value = '';
    renderSteps();
    deformationChanged();
  };
}

function renderSteps(): void {
  const list = $('steps');
  list.replaceChildren(
    ...state.steps.map((step, i) => {
      const li = el('li', { className: pickingStep === i ? 'picking' : '' });
      const move = (d: number) => {
        const j = i + d;
        if (j < 0 || j >= state.steps.length) return;
        [state.steps[i], state.steps[j]] = [state.steps[j], state.steps[i]];
        pickingStep = null;
        renderSteps();
        deformationChanged();
      };
      const up = el('button', { type: 'button', textContent: '↑', title: 'Earlier', disabled: i === 0 });
      const down = el('button', { type: 'button', textContent: '↓', title: 'Later', disabled: i === state.steps.length - 1 });
      const remove = el('button', { type: 'button', textContent: '✕', title: 'Remove' });
      up.onclick = () => move(-1);
      down.onclick = () => move(1);
      remove.onclick = () => {
        state.steps.splice(i, 1);
        pickingStep = null;
        renderSteps();
        deformationChanged();
      };

      const select = el('select');
      const kind = el('span', { className: 'kind', textContent: step.kind === 'knob' ? 'consonant' : step.kind === 'vowels' ? 'vowels' : 'accent' });
      const body = el('div', { className: 'step-params' });
      if (step.kind === 'accent') {
        select.append(...catalog.accents.map((a) => el('option', { value: a.name, textContent: a.name, selected: a.name === step.name })));
        select.onchange = () => {
          step.name = select.value;
          renderSteps();
          deformationChanged();
        };
        const preset = catalog.accents.find((a) => a.name === step.name);
        body.append(el('p', { className: 'hint', textContent: preset?.description ?? '' }));
      } else if (step.kind === 'knob') {
        select.append(...catalog.knobs.map((k) => el('option', { value: k, textContent: k, selected: k === step.name })));
        select.onchange = () => {
          step.name = select.value;
          renderSteps();
          deformationChanged();
        };
        body.append(el('p', { className: 'hint', textContent: KNOB_EXAMPLES[step.name] ?? '' }));
      } else {
        select.append(...Object.entries(MOVES).map(([k, m]) => el('option', { value: k, textContent: m.label, selected: k === step.move })));
        select.onchange = () => {
          Object.assign(step, newStep(`vowels:${select.value}`));
          pickingStep = null;
          renderSteps();
          deformationChanged();
        };
        const kindInfo = MOVES[step.move];
        kindInfo.params.forEach((p, k) => body.append(paramControl(step, p, k)));
        if (kindInfo.masked) {
          const mask = el(
            'select',
            {},
            ...MASKS.map((m) => el('option', { value: m, textContent: m ? `only ${m} vowels` : 'every vowel', selected: m === step.mask })),
          );
          mask.onchange = () => {
            step.mask = mask.value as Mask;
            deformationChanged();
          };
          body.append(el('label', { className: 'row' }, 'apply to ', mask));
        }
        if (step.move === 'pull') {
          const pick = el('button', { type: 'button', textContent: pickingStep === i ? 'click the chart…' : 'pick the point on the chart' });
          pick.onclick = () => {
            pickingStep = pickingStep === i ? null : i;
            renderSteps();
            renderChart();
          };
          body.append(pick);
        }
        body.append(el('p', { className: 'hint', textContent: stepDto(step).value }));
      }
      li.append(el('div', { className: 'step-head' }, kind, select, up, down, remove), body);
      return li;
    }),
  );
  if (!state.steps.length) list.append(el('li', { className: 'hint', textContent: 'No steps: the line is heard as spoken.' }));
  renderDeformToggle();
}

function paramControl(step: Extract<Step, { kind: 'vowels' }>, p: Param, k: number): HTMLElement {
  const range = el('input', { type: 'range', min: String(p.min), max: String(p.max), step: String(p.step), value: String(step.params[k]) });
  const number = el('input', { type: 'number', step: String(p.step), value: String(step.params[k]) });
  const set = (v: number) => {
    if (!Number.isFinite(v)) return;
    step.params[k] = v;
    range.value = number.value = String(v);
    const spec = range.closest('.step-params')?.querySelector('.hint:last-child');
    if (spec) spec.textContent = stepDto(step).value;
    deformationChanged();
  };
  range.addEventListener('input', () => set(Number(range.value)));
  number.addEventListener('change', () => set(Number(number.value)));
  const wrap = el('div', { className: 'knob' }, el('div', { className: 'knob-head' }, el('span', { className: 'name', textContent: p.name })), el('div', { className: 'knob-inputs' }, range, number));
  return wrap;
}

// ---------------------------------------------------------------- chart ----

// Backness on x (front left), height on y (high up), as vowel charts are drawn.
const CHART = { w: 320, h: 280, pad: 24, x0: -0.15, x1: 1.15, y0: -0.15, y1: 1.15 };
const CENTER: [number, number] = [0.58, 0.59];
const px = (x: number) => CHART.pad + ((x - CHART.x0) / (CHART.x1 - CHART.x0)) * (CHART.w - 2 * CHART.pad);
const py = (y: number) => CHART.pad + ((CHART.y1 - y) / (CHART.y1 - CHART.y0)) * (CHART.h - 2 * CHART.pad);
const unpx = (sx: number) => CHART.x0 + ((sx - CHART.pad) / (CHART.w - 2 * CHART.pad)) * (CHART.x1 - CHART.x0);
const unpy = (sy: number) => CHART.y1 - ((sy - CHART.pad) / (CHART.h - 2 * CHART.pad)) * (CHART.y1 - CHART.y0);

let preview: PreviewDto | null = null;

function renderChart(): void {
  const chart = $('vowel-chart') as unknown as SVGSVGElement;
  chart.classList.toggle('pick', pickingStep !== null);
  const parts: SVGElement[] = [];
  parts.push(
    svg('defs', {}),
    svg('rect', { x: px(0), y: py(1), width: px(1) - px(0), height: py(0) - py(1), class: 'grid', fill: 'none' }),
    svg('line', { x1: px(0.5), y1: py(0), x2: px(0.5), y2: py(1), class: 'grid', 'stroke-dasharray': '2 3' }),
    svg('line', { x1: px(0), y1: py(0.5), x2: px(1), y2: py(0.5), class: 'grid', 'stroke-dasharray': '2 3' }),
    svg('text', { x: px(0), y: CHART.h - 6, class: 'axis' }, 'front'),
    svg('text', { x: px(1), y: CHART.h - 6, class: 'axis', 'text-anchor': 'end' }, 'back'),
    svg('text', { x: 4, y: py(1) + 3, class: 'axis' }, 'high'),
    svg('text', { x: 4, y: py(0) + 3, class: 'axis' }, 'low'),
    svg('path', { d: `M${px(CENTER[0]) - 4},${py(CENTER[1])}h8M${px(CENTER[0])},${py(CENTER[1]) - 4}v8`, class: 'grid' }),
  );
  const defs = parts[0];
  for (const [id, cls] of [
    ['arrow-orig', 'orig'],
    ['arrow-moved', 'moved'],
  ]) {
    const marker = svg('marker', { id, viewBox: '0 0 6 6', refX: 5, refY: 3, markerWidth: 6, markerHeight: 6, orient: 'auto-start-reverse' });
    marker.append(svg('path', { d: 'M0,0L6,3L0,6z', class: cls }));
    defs.append(marker);
  }
  const changes: string[] = [];
  for (const v of preview?.vowels ?? []) {
    const [sx, sy] = v.start;
    const [ex, ey] = v.end;
    const glide = sx !== ex || sy !== ey;
    if (glide) parts.push(svg('line', { x1: px(sx), y1: py(sy), x2: px(ex), y2: py(ey), class: 'orig', 'stroke-width': 1, 'marker-end': 'url(#arrow-orig)' }));
    parts.push(svg('circle', { cx: px(sx), cy: py(sy), r: 2.5, class: 'orig' }));
    parts.push(svg('text', { x: px(sx) + 4, y: py(sy) - 4, class: 'orig-label' }, v.symbol));
    const [mx, my] = v.moved_start;
    const [nx, ny] = v.moved_end;
    const moved = mx !== sx || my !== sy || nx !== ex || ny !== ey;
    if (!moved) continue;
    parts.push(svg('line', { x1: px(sx), y1: py(sy), x2: px(mx), y2: py(my), class: 'moved', 'stroke-width': 1, 'stroke-dasharray': '3 2', 'stroke-opacity': 0.6 }));
    if (glide) parts.push(svg('line', { x1: px(mx), y1: py(my), x2: px(nx), y2: py(ny), class: 'moved', 'stroke-width': 1.25, 'marker-end': 'url(#arrow-moved)' }));
    parts.push(svg('circle', { cx: px(mx), cy: py(my), r: 3, class: 'moved' }));
    if (v.becomes !== v.symbol) {
      parts.push(svg('text', { x: px(mx) + 4, y: py(my) + 12, class: 'moved-label' }, `${v.symbol}→${v.becomes}`));
      changes.push(`${v.symbol} → ${v.becomes}`);
    }
  }
  state.steps.forEach((step, i) => {
    if (step.kind !== 'vowels' || step.move !== 'pull') return;
    const [x, y] = step.params;
    parts.push(svg('circle', { cx: px(x), cy: py(y), r: 6, class: 'target' }));
    parts.push(svg('path', { d: `M${px(x) - 9},${py(y)}h18M${px(x)},${py(y) - 9}v18`, class: 'target' }));
    parts.push(svg('text', { x: px(x) + 8, y: py(y) - 8, class: 'axis' }, `pull ${i + 1}`));
  });
  chart.replaceChildren(...parts);
  $('vowel-changes').textContent = changes.length
    ? `Snaps to: ${changes.join(', ')}`
    : preview?.vowels.some((v) => v.moved_start[0] !== v.start[0] || v.moved_start[1] !== v.start[1])
      ? 'The moves shift vowels, but each still snaps to itself.'
      : 'No vowel-space moves.';
}

function onChartClick(event: MouseEvent): void {
  if (pickingStep === null) return;
  const step = state.steps[pickingStep];
  if (step?.kind !== 'vowels' || step.move !== 'pull') return;
  const chart = event.currentTarget as SVGSVGElement;
  const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(chart.getScreenCTM()!.inverse());
  step.params[0] = Math.round(unpx(point.x) * 100) / 100;
  step.params[1] = Math.round(unpy(point.y) * 100) / 100;
  pickingStep = null;
  renderSteps();
  deformationChanged();
}

// -------------------------------------------------------------- results ----

type Op = { op: 'same' | 'del' | 'ins'; ph: string };

/** The deformation's edits to one word, by longest common subsequence. */
function diff(a: string[], b: string[]): Op[] {
  const n = a.length;
  const m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      ops.push({ op: 'same', ph: a[i] });
      i++;
      j++;
    } else if (i < n && (j === m || lcs[i + 1][j] >= lcs[i][j + 1])) {
      ops.push({ op: 'del', ph: a[i++] });
    } else {
      ops.push({ op: 'ins', ph: b[j++] });
    }
  }
  return ops;
}

function phonemeLine(ops: Op[]): HTMLElement {
  const span = el('span', { className: 'ph' });
  ops.forEach((o, k) => {
    if (k) span.append(' ');
    span.append(o.op === 'same' ? o.ph : el(o.op, { textContent: o.ph }));
  });
  return span;
}

let selectedHearing = 0;
let lastResult: ResultDto | null = null;

function renderResult(result: ResultDto, ms: number, rebuildMs: number): void {
  lastResult = result;
  showError('');
  const mishear = result.words.length > 0;
  // Sounds.
  $('sounds-section').hidden = !mishear;
  const changedInput: boolean[] = [];
  if (mishear) {
    $('deformation-name').textContent = result.deformation ? `Deformation: ${result.deformation}` : 'No deformation.';
    const rows = result.words.map((w) => {
      const ops = diff(w.phonemes, w.deformed);
      for (const o of ops) if (o.op !== 'del') changedInput.push(o.op === 'ins');
      return el(
        'tr',
        {},
        el('td', { textContent: w.text }),
        el('td', {}, el('span', { className: 'ph', textContent: w.phonemes.join(' ') })),
        el('td', {}, phonemeLine(ops)),
      );
    });
    $('sounds').replaceChildren(el('tr', {}, el('th', { textContent: 'word' }), el('th', { textContent: 'spoken' }), el('th', { textContent: 'heard as' })), ...rows);
  }
  // Hearings.
  $('heard-section').hidden = false;
  selectedHearing = Math.min(selectedHearing, Math.max(0, result.hearings.length - 1));
  const list = $('hearings');
  list.replaceChildren(
    ...result.hearings.map((h, i) => {
      const li = el(
        'li',
        { className: i === selectedHearing ? 'selected' : '' },
        el('span', { className: 'text', textContent: h.text }),
        el('span', {
          className: 'meta',
          textContent: [
            `cost ${fmt(h.cost)}`,
            `sound ${fmt(h.phonetic)}`,
            ...(mishear ? [`${h.shared} same word`, `${h.related} same root`] : []),
            ...(result.boundaries.length ? [`crosses ${result.boundaries.length - h.kept} of ${result.boundaries.length} boundaries`] : []),
          ].join(' · '),
          title:
            'cost: sound + word prior + word penalty + novelty + same-root + kept boundaries. sound: the phonetic part alone. same root: words other than the line’s own that share a stem with one of them.',
        }),
      );
      li.onclick = () => {
        selectedHearing = i;
        list.querySelectorAll('li').forEach((x, k) => x.classList.toggle('selected', k === i));
        $('alignment')
          .querySelectorAll<HTMLElement>('.chip')
          .forEach((c) => c.classList.toggle('selected', c.dataset.row === String(i)));
      };
      return li;
    }),
  );
  if (!result.hearings.length) list.append(el('li', { className: 'hint', textContent: 'Nothing within the bound. Raise “max bound” or “only within cost”.' }));
  const s = result.stats;
  const parts = [
    `${result.hearings.length} in ${thousands(ms)} ms`,
    rebuildMs ? `(lexicon rebuilt in ${thousands(rebuildMs)} ms)` : '',
    `bound ${fmt(s.bound, 1)} after ${s.rounds} round${s.rounds === 1 ? '' : 's'}`,
    `${thousands(s.nodes)} trie nodes`,
    `${thousands(s.edges)} edges`,
  ];
  $('stats').textContent = parts.filter(Boolean).join(' · ');
  renderAlignment(result, changedInput);
}

function renderAlignment(result: ResultDto, changedInput: boolean[]): void {
  const grid = $('alignment');
  const n = result.input.length;
  grid.style.gridTemplateColumns = `auto repeat(${n}, max-content)`;
  const cells: HTMLElement[] = [];
  const label = (text: string, row: number) => {
    const node = el('div', { className: 'label', textContent: text });
    node.style.gridRow = String(row);
    node.style.gridColumn = '1';
    cells.push(node);
  };
  const place = (node: HTMLElement, row: number, from: number, to: number) => {
    node.style.gridRow = String(row);
    node.style.gridColumn = `${from + 2} / ${Math.max(to, from + 1) + 2}`;
    cells.push(node);
  };
  let row = 1;
  if (result.words.length) {
    label('line', row);
    let at = 0;
    for (const w of result.words) {
      if (w.deformed.length) place(el('div', { className: 'orig-word', textContent: w.text }), row, at, at + w.deformed.length);
      at += w.deformed.length;
    }
    row++;
  }
  label(result.words.length ? 'heard as' : 'input', row);
  result.input.forEach((ph, k) => place(el('div', { className: `input ph${changedInput[k] ? ' changed' : ''}`, textContent: ph }), row, k, k + 1));
  row++;
  const lineWords = new Set(result.words.map((w) => w.text));
  const firstHearingRow = row;
  result.hearings.forEach((h, i) => {
    label(`${i + 1}`, row);
    for (const w of h.words) {
      const chip = el('div', {
        className: `chip${lineWords.has(w.text) ? ' shared' : ''}${i === selectedHearing ? ' selected' : ''}`,
        textContent: w.text,
        title: result.input.slice(w.start, w.end).join(' '),
      });
      chip.dataset.row = String(i);
      place(chip, row, w.start, w.end);
    }
    row++;
  });
  // The line's word boundaries, drawn over the hearings: a heard word they
  // pass through cuts across one.
  for (const b of result.boundaries) {
    const line = el('div', { className: 'boundary' });
    line.style.gridRow = `1 / ${Math.max(row, firstHearingRow + 1)}`;
    line.style.gridColumn = String(b + 2);
    cells.push(line);
  }
  grid.replaceChildren(...cells);
}

// ---------------------------------------------------------------- sweep ----

function sweepKnobs(): { id: string; label: string; apply: (s: State, v: number) => void }[] {
  const numbered = (group: 'search' | 'cost', knobs: Knob[]) =>
    knobs
      .filter((k): k is NumberKnob => k.type === 'number')
      .map((k) => ({
        id: `${group}.${k.key}`,
        label: `${group}: ${k.label}`,
        apply: (s: State, v: number) => {
          (s[group] as unknown as Record<string, number>)[k.key] = v;
        },
      }));
  return [
    ...numbered('search', SEARCH_KNOBS),
    ...numbered('cost', COST_KNOBS),
    { id: 'deformation.intensity', label: 'deformation: intensity', apply: (s, v) => (s.deformation.intensity = v) },
    { id: 'deformation.seed', label: 'deformation: seed', apply: (s, v) => (s.deformation.seed = v) },
  ];
}

function renderSweepControls(): void {
  const select = $<HTMLSelectElement>('sweep-knob');
  select.replaceChildren(...sweepKnobs().map((k) => el('option', { value: k.id, textContent: k.label, selected: k.id === state.sweepKnob })));
  select.onchange = () => {
    state.sweepKnob = select.value;
    saveState();
  };
  const values = $<HTMLInputElement>('sweep-values');
  values.value = state.sweepValues;
  values.onchange = () => {
    state.sweepValues = values.value;
    saveState();
  };
  $('sweep-run').onclick = runSweep;
}

async function runSweep(): Promise<void> {
  const knob = sweepKnobs().find((k) => k.id === state.sweepKnob);
  const values = state.sweepValues
    .split(/[,\s]+/)
    .filter(Boolean)
    .map(Number)
    .filter(Number.isFinite);
  if (!knob || !values.length) return;
  const section = $('sweep-section');
  section.hidden = false;
  $('sweep-title').textContent = `Sweep: ${knob.label}`;
  const columns = values.map((v) => el('div', { className: 'column' }, el('h4', { textContent: `${v}` }), el('p', { className: 'hint', textContent: '…' })));
  $('sweep').replaceChildren(...columns);
  section.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  for (const [k, v] of values.entries()) {
    setStatus(`Sweep ${k + 1} of ${values.length}…`, true);
    const s: State = structuredClone(state);
    knob.apply(s, v);
    const saved = state;
    state = s;
    const req = request();
    state = saved;
    const reply = await call({ type: 'run', id: nextId++, lexicon: s.lexicon, request: req });
    const column = columns[k];
    if (reply.type === 'error') {
      column.replaceChildren(column.firstChild!, el('p', { className: 'error', textContent: reply.message }));
    } else if (reply.type === 'result') {
      column.replaceChildren(
        column.firstChild!,
        el(
          'ol',
          {},
          ...reply.result.hearings.map((h) => el('li', {}, h.text, el('span', { className: 'cost', textContent: fmt(h.cost) }))),
        ),
        el('p', { className: 'hint', textContent: `${thousands(reply.ms)} ms` }),
      );
    }
  }
  setStatus('');
}

// ------------------------------------------------------------- running ----

let lastLexicon: LexiconOptions | null = null;
const sameLexicon = () => same(state.lexicon, lastLexicon);
let timer: number | undefined;
let running = false;
let rerun = false;

function changed(): void {
  saveState();
  if (history.replaceState && location.hash) history.replaceState(null, '', location.pathname);
  if (state.autoRun) {
    clearTimeout(timer);
    timer = window.setTimeout(run, 250);
  }
}

function deformationChanged(): void {
  updatePreview();
  changed();
}

async function updatePreview(): Promise<void> {
  const reply = await call({ type: 'preview', id: nextId++, deformation: stepsDto() });
  if (reply.type === 'preview') {
    preview = reply.preview;
    renderChart();
  }
}

async function run(): Promise<void> {
  clearTimeout(timer);
  if (running) {
    rerun = true;
    return;
  }
  running = true;
  do {
    rerun = false;
    const text = state.mode === 'mishear' ? state.text : state.phonemes;
    if (!text.trim()) break;
    setStatus(sameLexicon() ? 'Hearing…' : 'Rebuilding the lexicon, then hearing…', true);
    const lexicon = structuredClone(state.lexicon);
    const reply = await call({ type: 'run', id: nextId++, lexicon, request: structuredClone(request()) });
    if (reply.type === 'result') lastLexicon = lexicon;
    if (rerun) continue; // Stale: the knobs moved while it ran.
    if (reply.type === 'result') {
      lexiconInfo = reply.info;
      renderLexiconInfo();
      renderResult(reply.result, reply.ms, reply.rebuildMs);
      setStatus(`Heard in ${thousands(reply.ms)} ms`);
    } else if (reply.type === 'error') {
      lastLexicon = lexicon;
      setStatus('');
      showError(reply.message);
    }
  } while (rerun);
  running = false;
}

function renderLexiconInfo(): void {
  if (!lexiconInfo) return;
  const i = lexiconInfo;
  $('lexicon-info').textContent =
    `${thousands(i.words)} words, ${thousands(i.trie_nodes)} trie nodes; word-pair counts ${i.bigrams ? 'loaded' : 'not loaded (fetched when bigrams are turned on)'}; ` +
    `ENABLE word list ${i.common_words_available ? 'loaded' : 'not found'}.`;
}

// ---------------------------------------------------------------- setup ----

function renderInput(): void {
  const text = $<HTMLTextAreaElement>('text');
  text.value = state.mode === 'mishear' ? state.text : state.phonemes;
  text.placeholder = state.mode === 'mishear' ? 'A line of verse' : 'ARPAbet, e.g. K IH1 S DH AH0 S K AY1';
  document.querySelectorAll<HTMLInputElement>('input[name="mode"]').forEach((r) => (r.checked = r.value === state.mode));
  $('deformation-panel').classList.toggle('disabled', state.mode === 'phonemes');
  $('deformation-panel').title = state.mode === 'phonemes' ? 'Phoneme input is heard as given: no deformation, no novelty.' : '';
  $<HTMLInputElement>('auto-run').checked = state.autoRun;
}

function renderDeformToggle(): void {
  $<HTMLInputElement>('deform').checked = state.deform;
  $('deformation-body').classList.toggle('off', !state.deform);
  $('deform-state').textContent = state.deform ? (state.steps.length ? '(on)' : '(on, no steps)') : '(off)';
}

function renderAll(): void {
  renderInput();
  renderDeformToggle();
  ($('sounds-section') as HTMLDetailsElement).open = state.soundsOpen;
  renderSteps();
  $('deformation-knobs').replaceChildren(...DEFORMATION_KNOBS.map((k) => knobControl('deformation', k, deformationChanged)));
  renderKnobs('search', SEARCH_KNOBS, $('search-knobs'));
  renderKnobs('cost', COST_KNOBS, $('cost-knobs'));
  renderKnobs('lexicon', LEXICON_KNOBS, $('lexicon-knobs'));
  renderSweepControls();
}

/** Rounds the f32 noise out of numbers from Rust (0.100000001 → 0.1). */
function clean<T>(value: T): T {
  if (typeof value === 'number') return Number(value.toPrecision(6)) as T;
  if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) (value as Record<string, unknown>)[k] = clean(v);
  }
  return value;
}

function onReady(m: Extract<FromWorker, { type: 'ready' }>): void {
  catalog = m.catalog;
  defaults = clean(m.defaults);
  lexiconInfo = m.info;
  state = loadState();
  lastLexicon = clean(m.lexicon);
  renderAddStep();
  renderAll();
  renderLexiconInfo();
  setStatus(`Loaded in ${thousands(m.loadMs)} ms`);

  const text = $<HTMLTextAreaElement>('text');
  text.addEventListener('input', () => {
    if (state.mode === 'mishear') state.text = text.value;
    else state.phonemes = text.value;
    $('source').textContent = '';
    changed();
  });
  text.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      run();
    }
  });
  document.querySelectorAll<HTMLInputElement>('input[name="mode"]').forEach((r) =>
    r.addEventListener('change', () => {
      state.mode = r.value as Mode;
      renderInput();
      changed();
    }),
  );
  $<HTMLInputElement>('auto-run').addEventListener('change', (e) => {
    state.autoRun = (e.target as HTMLInputElement).checked;
    saveState();
  });
  $('run').onclick = run;
  $('random-line').onclick = () => {
    const pick = LINES[Math.floor(Math.random() * LINES.length)];
    if (!pick) return;
    state.mode = 'mishear';
    state.text = pick.line;
    renderInput();
    $('source').textContent = `— ${pick.poem}`;
    changed();
    if (!state.autoRun) run();
  };
  $('reset-all').onclick = () => {
    const keep = { text: state.text, phonemes: state.phonemes, mode: state.mode };
    state = { ...defaultState(), ...keep };
    pickingStep = null;
    renderAll();
    deformationChanged();
  };
  $('copy-link').onclick = async () => {
    const url = `${location.origin}${location.pathname}#${encodeURIComponent(JSON.stringify(savedState()))}`;
    await navigator.clipboard.writeText(url);
    setStatus('Link copied.');
  };
  $('vowel-chart').addEventListener('click', onChartClick as EventListener);
  $<HTMLInputElement>('deform').addEventListener('change', (e) => {
    state.deform = (e.target as HTMLInputElement).checked;
    renderDeformToggle();
    deformationChanged();
  });
  $('sounds-section').addEventListener('toggle', () => {
    state.soundsOpen = ($('sounds-section') as HTMLDetailsElement).open;
    saveState();
  });
  updatePreview();
  run();
}

setStatus('Starting the engine…', true);
// Build the saved lexicon from the start, so the first run needn't rebuild it.
let savedLexicon: Partial<LexiconOptions> = {};
try {
  savedLexicon = (unsaved(readSaved()?.lexicon) as Partial<LexiconOptions>) ?? {};
} catch {
  /* the defaults, then */
}
worker.postMessage({ type: 'init', lexicon: savedLexicon } satisfies ToWorker);
