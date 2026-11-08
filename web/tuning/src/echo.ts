import init, {
	Analyzer,
	echoConfigFor,
	echoThreshold,
	type AlignerKindDto,
	type EchoAlignmentDto,
	type EchoConfigDto,
	type EchoNormalizationDto,
	type EchoStrictnessDto,
} from "mime-reader";
import echoPairTests from "../../../echo_pair_tests.json";
import {
	configIsModified,
	configOverrides,
	isRecord,
	renderConfigControls,
	restoreConfig,
} from "./config-controls";
import { fixtureGroupsFrom, type FixtureGroup, type PairTuple } from "./fixtures";

// The dictionary is served as a static asset (see web/tuning/public/).
const DICT_URL = "/cmudict-0.7b";
const STORAGE_KEY = "mime_reader:tuning:echo-config:v2";

const ALIGNERS: Array<{ kind: AlignerKindDto; short: string }> = [
	{ kind: "semi_global", short: "SG" },
	{ kind: "waterman_eggert", short: "WE" },
];
type AlignerChoice = AlignerKindDto | "all";
type PerAligner<T> = Record<AlignerKindDto, T>;

type NumericField = Exclude<keyof EchoConfigDto, "normalization" | "aligner">;

const ECHO_CONFIG_FIELDS: NumericField[] = [
	"vowel_coefficient",
	"consonant_coefficient",
	"vowel_zero_point",
	"consonant_zero_point",
	"primary_weight",
	"secondary_weight",
	"unstressed_weight",
	"gap_open",
	"gap_extend",
	"gap_open_repeated_consonant",
	"move_cost",
	"run_open",
	"min_run",
	"max_runs",
	"max_variants_per_line",
	"max_variant_pairs",
];

// From the doc comments on `EchoConfig` and its parts (src/echo.rs,
// src/echo/similarity.rs). Keep in sync — tsify doesn't carry Rust docs.
const ECHO_CONFIG_DOCS: Record<NumericField, string> = {
	vowel_coefficient:
		"Multiplier on vowel similarity. Echo's own coefficient: it never reads the rhyme config's 7:1.",
	consonant_coefficient: "Multiplier on consonant similarity.",
	vowel_zero_point:
		"Vowel distance (0-1000) at which a vowel pair scores 0. The default is chance: the expected distance between two random CMUdict vowels (376). Closer pairs score positive, farther ones negative.",
	consonant_zero_point:
		"Consonant distance (0-1000) at which a consonant pair scores 0. Tuned to 300, well below chance (789): at chance, alignments that skip material collect chance consonant matches. A high value makes mismatched consonants cheap.",
	primary_weight:
		"Weight of a primary-stressed vowel (and of every consonant). A pair takes the smaller weight of its two vowels, so nothing beats a verbatim repeat.",
	secondary_weight: "Weight of a secondary-stressed vowel.",
	unstressed_weight: "Weight of an unstressed vowel.",
	gap_open:
		"Thousandths of a unit (the similarity of an identical primary vowel). Opening a gap: an inserted or deleted phrase costs this once.",
	gap_extend:
		"Thousandths of a unit, per gapped phoneme. 0 makes an insertion one event however long.",
	gap_open_repeated_consonant:
		"Thousandths of a unit. Opening a gap at a consonant that repeats its predecessor.",
	move_cost: "Thousandths of a unit. A moved block (Waterman–Eggert).",
	run_open: "Thousandths of a unit. Each local run after the first (Waterman–Eggert).",
	min_run:
		"Thousandths of a unit. After the first, local runs scoring less end the search (Waterman–Eggert).",
	max_runs: "At most this many local runs (Waterman–Eggert).",
	max_variants_per_line:
		"A line with more pronunciation combinations is compared under its first pronunciations only.",
	max_variant_pairs: "At most this many (variant, variant) pairs per line pair.",
};

type Expected = "echo" | "not" | "unlabeled";

interface Pair {
	a: string;
	b: string;
	group?: string;
}

interface EchoGroup extends FixtureGroup {
	expected: Expected;
	class: number | null;
	comment: string;
}

type Outcome = { ok: true; dto: EchoAlignmentDto } | { ok: false; message: string };

const $ = <T extends HTMLElement>(sel: string): T =>
	document.querySelector(sel) as T;

const FIXTURE_GROUPS: EchoGroup[] = fixtureGroupsFrom(echoPairTests).map((g) => ({
	...g,
	expected: isExpected(g.meta.expected) ? g.meta.expected : "unlabeled",
	class: typeof g.meta.class === "number" ? g.meta.class : null,
	comment: typeof g.meta.comment === "string" ? g.meta.comment : "",
}));

function isExpected(value: unknown): value is Expected {
	return value === "echo" || value === "not" || value === "unlabeled";
}

// Each aligner has its own tuned config (its preset) and its own thresholds;
// the config aside edits one of them at a time.
let analyzer: Analyzer;
let presets: PerAligner<EchoConfigDto>;
let configs: PerAligner<EchoConfigDto>;
let editing: AlignerKindDto = "semi_global";
let alignerChoice: AlignerChoice = "all";
let strictness: EchoStrictnessDto = "strict";
const expanded = new Set<number>();
const savedPairs: Pair[] = FIXTURE_GROUPS.flatMap(({ key, pairs }) =>
	pairs.map(([a, b]: PairTuple) => ({ a, b, group: key })),
);

function perAligner<T>(value: (kind: AlignerKindDto) => T): PerAligner<T> {
	return Object.fromEntries(ALIGNERS.map(({ kind }) => [kind, value(kind)])) as PerAligner<T>;
}

async function main(): Promise<void> {
	await init();
	const res = await fetch(DICT_URL);
	if (!res.ok) throw new Error(`failed to fetch dictionary (HTTP ${res.status})`);
	analyzer = new Analyzer(new Uint8Array(await res.arrayBuffer()), "allow_derived");
	presets = perAligner((kind) => echoConfigFor(kind));

	const stored = loadStored();
	const storedConfigs = isRecord(stored?.configs) ? stored.configs : {};
	configs = perAligner((kind) => {
		const saved = storedConfigs[kind];
		const config = restoreConfig(presets[kind], ECHO_CONFIG_FIELDS, saved);
		if (isRecord(saved) && isNormalization(saved.normalization)) {
			config.normalization = saved.normalization;
		}
		return config;
	});
	if (isAlignerChoice(stored?.aligner)) alignerChoice = stored.aligner;
	if (isAligner(stored?.editing)) editing = stored.editing;
	if (isStrictness(stored?.strictness)) strictness = stored.strictness;
	if (typeof stored?.threshold === "number") {
		$<HTMLInputElement>("#threshold").value = String(stored.threshold);
	}

	buildAlignerChoice();
	buildStrictnessControl();
	buildConfigAligner();
	buildNormalizationControl();
	buildConfigControls();
	$<HTMLButtonElement>("#restore-defaults").addEventListener("click", restoreDefaults);
	syncRestoreDefaults();
	syncThresholdHint();
	for (const sel of ["#line-a", "#line-b"]) $(sel).addEventListener("input", render);
	$("#threshold").addEventListener("input", () => {
		persist();
		syncThresholdHint();
		render();
	});
	$<HTMLButtonElement>("#save").addEventListener("click", savePair);

	$("#status").hidden = true;
	$("main").hidden = false;
	render();
}

// ---- state & storage ------------------------------------------------------

function isNormalization(value: unknown): value is EchoNormalizationDto {
	return value === "shorter_self_similarity" || value === "mean_self_similarity";
}

function isAligner(value: unknown): value is AlignerKindDto {
	return ALIGNERS.some(({ kind }) => kind === value);
}

function isAlignerChoice(value: unknown): value is AlignerChoice {
	return value === "all" || isAligner(value);
}

function isStrictness(value: unknown): value is EchoStrictnessDto {
	return value === "strict" || value === "loose";
}

function loadStored(): Record<string, unknown> | null {
	try {
		const raw = window.localStorage.getItem(STORAGE_KEY);
		if (!raw) return null;
		const parsed: unknown = JSON.parse(raw);
		return isRecord(parsed) && parsed.version === 2 ? parsed : null;
	} catch {
		return null;
	}
}

// What differs from `kind`'s preset: its numeric overrides, plus the
// normalization when changed.
function overridesOf(kind: AlignerKindDto): Record<string, number | string> {
	const config = configs[kind];
	const preset = presets[kind];
	return {
		...configOverrides(config, preset, ECHO_CONFIG_FIELDS),
		...(config.normalization === preset.normalization
			? {}
			: { normalization: config.normalization }),
	};
}

function persist(): void {
	try {
		window.localStorage.setItem(
			STORAGE_KEY,
			JSON.stringify({
				version: 2,
				configs: perAligner(overridesOf),
				aligner: alignerChoice,
				editing,
				strictness,
				threshold: thresholdOverride(),
			}),
		);
	} catch {
		// Storage may be unavailable in private or restricted browser contexts.
	}
}

function configModified(kind: AlignerKindDto): boolean {
	return (
		configIsModified(configs[kind], presets[kind], ECHO_CONFIG_FIELDS) ||
		configs[kind].normalization !== presets[kind].normalization
	);
}

function syncRestoreDefaults(): void {
	$<HTMLButtonElement>("#restore-defaults").disabled = !configModified(editing);
}

function restoreDefaults(): void {
	configs[editing] = { ...presets[editing] };
	onConfigChange();
	buildConfigControls();
	syncNormalization();
}

// The typed threshold, or null to use each aligner's measured one.
function thresholdOverride(): number | null {
	const raw = $<HTMLInputElement>("#threshold").value.trim();
	if (raw === "") return null;
	const value = Number(raw);
	return Number.isFinite(value) ? value : null;
}

function thresholdFor(kind: AlignerKindDto): number {
	return thresholdOverride() ?? echoThreshold(strictness, kind);
}

function selectedAligners(): AlignerKindDto[] {
	return alignerChoice === "all" ? ALIGNERS.map(({ kind }) => kind) : [alignerChoice];
}

// ---- controls ---------------------------------------------------------------

function buildAlignerChoice(): void {
	for (const input of document.querySelectorAll<HTMLInputElement>('input[name="aligner"]')) {
		input.checked = input.value === alignerChoice;
		input.addEventListener("change", () => {
			if (!input.checked || !isAlignerChoice(input.value)) return;
			alignerChoice = input.value;
			// Choosing one aligner edits its config too.
			if (alignerChoice === "all") persist();
			else setEditing(alignerChoice);
			render();
		});
	}
}

function buildStrictnessControl(): void {
	const select = $<HTMLSelectElement>("#strictness");
	select.value = strictness;
	select.addEventListener("change", () => {
		if (!isStrictness(select.value)) return;
		strictness = select.value;
		persist();
		syncThresholdHint();
		render();
	});
}

// Which thresholds apply: the measured ones, which assume the tuned configs,
// or the typed override.
function syncThresholdHint(): void {
	const hint = $("#threshold-hint");
	if (thresholdOverride() !== null) {
		hint.textContent = "for every aligner";
		hint.title = "";
		return;
	}
	hint.textContent = ALIGNERS.map(
		({ kind, short }) => `${short} ${echoThreshold(strictness, kind).toFixed(2)}`,
	).join(" · ");
	hint.title = "Measured on the labeled corpus under each aligner's tuned config";
}

function buildConfigAligner(): void {
	for (const button of document.querySelectorAll<HTMLButtonElement>("#config-aligner button")) {
		button.addEventListener("click", () => {
			if (isAligner(button.dataset.kind)) setEditing(button.dataset.kind);
		});
	}
	syncConfigAligner();
}

function syncConfigAligner(): void {
	for (const button of document.querySelectorAll<HTMLButtonElement>("#config-aligner button")) {
		button.setAttribute("aria-selected", String(button.dataset.kind === editing));
	}
}

function setEditing(kind: AlignerKindDto): void {
	editing = kind;
	persist();
	syncConfigAligner();
	buildConfigControls();
	syncNormalization();
	syncRestoreDefaults();
}

function buildNormalizationControl(): void {
	const select = $<HTMLSelectElement>("#normalization");
	select.addEventListener("change", () => {
		if (!isNormalization(select.value)) return;
		configs[editing].normalization = select.value;
		onConfigChange();
		syncNormalization();
	});
	syncNormalization();
}

function syncNormalization(): void {
	const modified = configs[editing].normalization !== presets[editing].normalization;
	$<HTMLSelectElement>("#normalization").value = configs[editing].normalization;
	$("#normalization-control").classList.toggle("modified", modified);
	$("#normalization-modified").hidden = !modified;
}

function onConfigChange(): void {
	persist();
	syncRestoreDefaults();
	render();
}

function buildConfigControls(): void {
	renderConfigControls(
		$("#config-controls"),
		configs[editing],
		presets[editing],
		ECHO_CONFIG_FIELDS,
		ECHO_CONFIG_DOCS,
		(field, value) => {
			configs[editing][field] = value;
		},
		onConfigChange,
	);
}

function savePair(): void {
	const pair = currentPair();
	if (!pair.a || !pair.b) return;
	savedPairs.unshift(pair);
	expanded.clear();
	$<HTMLInputElement>("#line-a").value = "";
	$<HTMLInputElement>("#line-b").value = "";
	$<HTMLInputElement>("#line-a").focus();
	render();
}

function currentPair(): Pair {
	return {
		a: $<HTMLInputElement>("#line-a").value.trim(),
		b: $<HTMLInputElement>("#line-b").value.trim(),
	};
}

// ---- scoring ----------------------------------------------------------------

function errorMessage(err: unknown): string {
	return String(err instanceof Error ? err.message : err);
}

function score(pair: Pair): Outcome {
	try {
		return { ok: true, dto: analyzer.scoreEcho(pair.a, pair.b) };
	} catch (err) {
		return { ok: false, message: errorMessage(err) };
	}
}

// Scores `pairs` with each of `kinds` under that aligner's config:
// result[pair][aligner]. A config the analyzer rejects (a value past its
// limits) fails every pair with the reason.
function scoreAll(pairs: Pair[], kinds: AlignerKindDto[]): Outcome[][] {
	const columns = kinds.map((kind): Outcome[] => {
		try {
			analyzer.setEchoConfig({ ...configs[kind], aligner: kind });
		} catch (err) {
			return pairs.map(() => ({ ok: false, message: errorMessage(err) }));
		}
		return pairs.map(score);
	});
	return pairs.map((_, i) => columns.map((column) => column[i]));
}

function expectedOf(pair: Pair): Expected | null {
	if (!pair.group) return null;
	return FIXTURE_GROUPS.find(({ key }) => key === pair.group)?.expected ?? null;
}

// Whether an outcome is right: an echo at or over the threshold, a non-echo
// under it. Unexpected-free pairs (live, saved) just report the threshold.
function verdict(outcome: Outcome, expected: Expected | null, t: number): boolean | null {
	if (!outcome.ok) return false;
	const over = outcome.dto.similarity >= t;
	if (expected === "unlabeled") return null;
	return expected === "not" ? !over : over;
}

// ---- rendering ---------------------------------------------------------------

function render(): void {
	const kinds = selectedAligners();
	const thresholds = kinds.map(thresholdFor);
	const live = currentPair();
	const hasLive = Boolean(live.a && live.b);
	const scored = scoreAll(hasLive ? [live, ...savedPairs] : savedPairs, kinds);
	const scores = hasLive ? scored.slice(1) : scored;
	renderOverview(scores, kinds, thresholds);

	const out = $("#output");
	out.replaceChildren();
	if (hasLive) out.append(pairRow(live, null, scored[0], kinds, thresholds));
	const groups: Array<{ label: string; detail: string; key: string | undefined }> = [
		{ label: "saved", detail: "", key: undefined },
		...FIXTURE_GROUPS.map((g) => ({
			label: g.label,
			detail: `class ${g.class ?? "—"} · expected ${g.expected}${g.comment ? ` · ${g.comment}` : ""}`,
			key: g.key,
		})),
	];
	for (const { label, detail, key } of groups) {
		const members = savedPairs
			.map((pair, index): [Pair, number] => [pair, index])
			.filter(([pair]) => pair.group === key);
		if (members.length === 0) continue;
		const heading = document.createElement("li");
		heading.className = "pair-group-heading";
		heading.textContent = `${label} (${members.length})`;
		if (detail) {
			const small = document.createElement("small");
			small.textContent = ` ${detail}`;
			heading.append(small);
		}
		out.append(heading);
		for (const [pair, index] of members) {
			out.append(pairRow(pair, index, scores[index], kinds, thresholds));
		}
	}
}

function shortName(kind: AlignerKindDto): string {
	return ALIGNERS.find((a) => a.kind === kind)?.short ?? kind;
}

function renderOverview(scores: Outcome[][], kinds: AlignerKindDto[], thresholds: number[]): void {
	const overview = $("#section-overview");
	overview.replaceChildren();
	for (const group of FIXTURE_GROUPS) {
		if (group.expected === "unlabeled") continue;
		const indexes = savedPairs
			.map((pair, index) => (pair.group === group.key ? index : -1))
			.filter((index) => index >= 0);
		const stat = document.createElement("span");
		stat.className = "section-stat";
		const counts = kinds.map((_, k) =>
			indexes.filter((i) => verdict(scores[i][k], group.expected, thresholds[k]) === true)
				.length,
		);
		stat.textContent = `${group.label}: ${kinds
			.map(
				(kind, k) =>
					`${kinds.length > 1 ? `${shortName(kind)} ` : ""}${counts[k]}/${indexes.length}`,
			)
			.join(" · ")}`;
		if (counts.every((c) => c === indexes.length)) stat.classList.add("all-pass");
		else if (counts.every((c) => c === 0)) stat.classList.add("all-fail");
		overview.append(stat);
	}
}

// `index` is the savedPairs index, or null for the live pair.
function pairRow(
	pair: Pair,
	index: number | null,
	outcomes: Outcome[],
	kinds: AlignerKindDto[],
	thresholds: number[],
): HTMLLIElement {
	const li = document.createElement("li");
	li.className = index === null ? "pair current" : "pair";

	const head = document.createElement("div");
	head.className = "pair-head";
	const words = document.createElement("span");
	words.className = "pair-words";
	words.textContent = `${pair.a} → ${pair.b}`;
	head.append(words);
	const expected = expectedOf(pair);
	outcomes.forEach((outcome, k) => head.append(chip(outcome, kinds[k], expected, thresholds[k])));

	const open = index === null || expanded.has(index);
	if (index !== null) {
		words.classList.add("toggle");
		words.title = open ? "hide alignments" : "show alignments";
		words.addEventListener("click", () => {
			if (expanded.has(index)) expanded.delete(index);
			else expanded.add(index);
			render();
		});
		if (!pair.group) {
			const remove = document.createElement("button");
			remove.type = "button";
			remove.className = "remove";
			remove.textContent = "✕";
			remove.title = "remove pair";
			remove.addEventListener("click", () => {
				savedPairs.splice(index, 1);
				expanded.clear();
				render();
			});
			head.append(remove);
		}
	}
	li.append(head);
	if (open) {
		outcomes.forEach((outcome) => {
			if (outcome.ok) li.append(alignmentEl(outcome.dto));
		});
	}
	return li;
}

function chip(outcome: Outcome, kind: AlignerKindDto, expected: Expected | null, t: number): HTMLElement {
	const b = document.createElement("span");
	b.className = "badge";
	const short = shortName(kind);
	if (!outcome.ok) {
		b.textContent = `${short} ⚠ ${outcome.message}`;
		b.classList.add("bad");
		return b;
	}
	const right = verdict(outcome, expected, t);
	const mark = right === null ? "·" : right ? "✓" : "✗";
	b.textContent = `${short} ${mark} ${outcome.dto.similarity.toFixed(3)}`;
	if (right !== null) b.classList.add(right ? "ok" : "bad");
	b.title =
		`${kind}: raw ${outcome.dto.raw} / self ${outcome.dto.self_a}, ${outcome.dto.self_b}` +
		` · threshold ${t.toFixed(2)}`;
	return b;
}

// One aligner's alignment: both lines as words of phoneme chips, tinted by
// the segment that aligned them (in place / moved) and greyed when
// unaligned; then each segment's steps, a over b with scores.
function alignmentEl(dto: EchoAlignmentDto): HTMLElement {
	const box = document.createElement("div");
	box.className = "echo-alignment";

	const moved = dto.segments.filter((s) => s.placement === "moved").length;
	const title = document.createElement("div");
	title.className = "echo-title";
	title.textContent =
		`${dto.aligner} · similarity ${dto.similarity.toFixed(3)} · raw ${dto.raw} ` +
		`(self ${dto.self_a} / ${dto.self_b}) · ${dto.segments.length} segment(s), ${moved} moved` +
		` · ${dto.variant_pairs_tried} variant pair(s)`;
	box.append(title);

	const segmentOf = (side: "a" | "b"): Map<number, number> => {
		const map = new Map<number, number>();
		for (const step of dto.steps) {
			const index = side === "a" ? step.a_index : step.b_index;
			const pairStep = step.a_index != null && step.b_index != null;
			if (index != null && pairStep && !map.has(index)) map.set(index, step.segment);
		}
		return map;
	};
	box.append(
		lineEl(dto.a_words, dto.a_phonemes, dto.a_word_starts, dto.a_aligned, segmentOf("a"), dto),
		lineEl(dto.b_words, dto.b_phonemes, dto.b_word_starts, dto.b_aligned, segmentOf("b"), dto),
	);

	dto.segments.forEach((segment, k) => {
		const strip = document.createElement("div");
		strip.className = `segment ${segment.placement}`;
		const label = document.createElement("span");
		label.className = "segment-label";
		label.textContent =
			`${k + 1} · ${segment.placement.replace("_", " ")} · ${segment.score}` +
			(segment.open_cost ? ` − ${segment.open_cost}` : "");
		strip.append(label);
		const cols = document.createElement("span");
		cols.className = "align";
		for (const step of dto.steps.filter((s) => s.segment === k)) {
			const col = document.createElement("span");
			col.className = `col ${step.step.kind}`;
			const top = document.createElement("span");
			const bottom = document.createElement("span");
			if (step.step.kind === "indel") {
				top.textContent = step.step.a ?? "–";
				bottom.textContent = step.step.b ?? "–";
			} else {
				top.textContent = step.step.a;
				bottom.textContent = step.step.b;
			}
			const cost = document.createElement("span");
			cost.className = "cost";
			cost.textContent = String(step.score);
			col.title = step.step.kind;
			col.append(top, bottom, cost);
			cols.append(col);
		}
		strip.append(cols);
		box.append(strip);
	});
	return box;
}

function lineEl(
	words: string[],
	phonemes: string[],
	wordStarts: number[],
	aligned: boolean[],
	segmentOf: Map<number, number>,
	dto: EchoAlignmentDto,
): HTMLElement {
	const line = document.createElement("div");
	line.className = "echo-line";
	wordStarts.forEach((start, w) => {
		const end = wordStarts[w + 1] ?? phonemes.length;
		const word = document.createElement("span");
		word.className = "echo-word";
		const label = document.createElement("span");
		label.className = "word-label";
		label.textContent = words[w] ?? "";
		const chips = document.createElement("span");
		chips.className = "word-phonemes";
		for (let i = start; i < end; i++) {
			const ph = document.createElement("span");
			ph.className = "ph";
			ph.textContent = phonemes[i];
			const segment = segmentOf.get(i);
			if (!aligned[i] || segment === undefined) {
				ph.classList.add("unaligned");
			} else {
				ph.classList.add(dto.segments[segment].placement);
				ph.dataset.segment = String(segment + 1);
				ph.title = `segment ${segment + 1} (${dto.segments[segment].placement.replace("_", " ")})`;
			}
			chips.append(ph);
		}
		word.append(label, chips);
		line.append(word);
	});
	return line;
}

main().catch((err) => {
	$("#status").textContent = `Error: ${err}`;
	$("#status").hidden = false;
});
