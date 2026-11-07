import init, {
	Analyzer,
	defaultEchoConfig,
	type AlignerKindDto,
	type EchoAlignmentDto,
	type EchoConfigDto,
	type EchoNormalizationDto,
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
const STORAGE_KEY = "mime_reader:tuning:echo-config:v1";
const DEFAULT_THRESHOLD = 0.5; // DEFAULT_ECHO_THRESHOLD, a placeholder

const ALIGNERS: Array<{ kind: AlignerKindDto; short: string }> = [
	{ kind: "semi_global", short: "SG" },
	{ kind: "waterman_eggert", short: "WE" },
	{ kind: "jump", short: "J" },
];
type AlignerChoice = AlignerKindDto | "all";

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
	"reuse_penalty",
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
		"Vowel distance (0-1000) at which a vowel pair scores 0: the expected distance between two random CMUdict vowels. Closer pairs score positive, farther ones negative.",
	consonant_zero_point:
		"Consonant distance (0-1000) at which a consonant pair scores 0, calibrated the same way. A high value makes mismatched consonants cheap.",
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
	move_cost:
		"Thousandths of a unit. A moved block (Waterman–Eggert) or a jump (jump DP).",
	run_open: "Thousandths of a unit. Each local run after the first (Waterman–Eggert).",
	min_run:
		"Thousandths of a unit. After the first, local runs scoring less end the search (Waterman–Eggert).",
	reuse_penalty:
		"Thousandths of a unit, per extra use of a phoneme (jump DP). 0 lets jump re-use material freely, which can push similarity above 1.",
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

let analyzer: Analyzer;
let defaults: EchoConfigDto;
let config: EchoConfigDto;
let alignerChoice: AlignerChoice = "all";
const expanded = new Set<number>();
const savedPairs: Pair[] = FIXTURE_GROUPS.flatMap(({ key, pairs }) =>
	pairs.map(([a, b]: PairTuple) => ({ a, b, group: key })),
);

async function main(): Promise<void> {
	await init();
	const res = await fetch(DICT_URL);
	if (!res.ok) throw new Error(`failed to fetch dictionary (HTTP ${res.status})`);
	analyzer = new Analyzer(new Uint8Array(await res.arrayBuffer()), "allow_derived");
	defaults = defaultEchoConfig();

	const stored = loadStored();
	config = restoreConfig(defaults, ECHO_CONFIG_FIELDS, stored?.config);
	if (isNormalization(stored?.normalization)) config.normalization = stored.normalization;
	if (isAlignerChoice(stored?.aligner)) alignerChoice = stored.aligner;
	if (typeof stored?.threshold === "number") {
		$<HTMLInputElement>("#threshold").value = String(stored.threshold);
	}
	analyzer.setEchoConfig(config);

	buildAlignerChoice();
	buildNormalizationControl();
	buildConfigControls();
	$<HTMLButtonElement>("#restore-defaults").addEventListener("click", restoreDefaults);
	syncRestoreDefaults();
	for (const sel of ["#line-a", "#line-b"]) $(sel).addEventListener("input", render);
	$("#threshold").addEventListener("input", () => {
		persist();
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

function isAlignerChoice(value: unknown): value is AlignerChoice {
	return value === "all" || ALIGNERS.some(({ kind }) => kind === value);
}

function loadStored(): Record<string, unknown> | null {
	try {
		const raw = window.localStorage.getItem(STORAGE_KEY);
		if (!raw) return null;
		const parsed: unknown = JSON.parse(raw);
		return isRecord(parsed) && parsed.version === 1 ? parsed : null;
	} catch {
		return null;
	}
}

function persist(): void {
	try {
		window.localStorage.setItem(
			STORAGE_KEY,
			JSON.stringify({
				version: 1,
				config: configOverrides(config, defaults, ECHO_CONFIG_FIELDS),
				normalization:
					config.normalization === defaults.normalization ? undefined : config.normalization,
				aligner: alignerChoice,
				threshold: threshold(),
			}),
		);
	} catch {
		// Storage may be unavailable in private or restricted browser contexts.
	}
}

function configModified(): boolean {
	return (
		configIsModified(config, defaults, ECHO_CONFIG_FIELDS) ||
		config.normalization !== defaults.normalization
	);
}

function syncRestoreDefaults(): void {
	$<HTMLButtonElement>("#restore-defaults").disabled = !configModified();
}

function restoreDefaults(): void {
	config = { ...defaults };
	analyzer.setEchoConfig(config);
	persist();
	buildConfigControls();
	syncNormalization();
	syncRestoreDefaults();
	render();
}

function threshold(): number {
	const value = Number($<HTMLInputElement>("#threshold").value.trim());
	return Number.isFinite(value) ? value : DEFAULT_THRESHOLD;
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
			persist();
			render();
		});
	}
}

function buildNormalizationControl(): void {
	const select = $<HTMLSelectElement>("#normalization");
	select.addEventListener("change", () => {
		if (!isNormalization(select.value)) return;
		config.normalization = select.value;
		analyzer.setEchoConfig(config);
		onConfigChange();
		syncNormalization();
	});
	syncNormalization();
}

function syncNormalization(): void {
	const modified = config.normalization !== defaults.normalization;
	$<HTMLSelectElement>("#normalization").value = config.normalization;
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
		config,
		defaults,
		ECHO_CONFIG_FIELDS,
		ECHO_CONFIG_DOCS,
		(field, value) => {
			config[field] = value;
			analyzer.setEchoConfig(config);
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

function score(pair: Pair, kind: AlignerKindDto): Outcome {
	try {
		return { ok: true, dto: analyzer.scoreEcho(pair.a, pair.b, kind) };
	} catch (err) {
		return { ok: false, message: String(err instanceof Error ? err.message : err) };
	}
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
	const t = threshold();
	const kinds = selectedAligners();
	const scores = savedPairs.map((pair) => kinds.map((kind) => score(pair, kind)));
	renderOverview(scores, kinds, t);

	const out = $("#output");
	out.replaceChildren();
	const live = currentPair();
	if (live.a && live.b) {
		out.append(pairRow(live, null, kinds.map((k) => score(live, k)), kinds, t));
	}
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
			out.append(pairRow(pair, index, scores[index], kinds, t));
		}
	}
}

function renderOverview(scores: Outcome[][], kinds: AlignerKindDto[], t: number): void {
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
			indexes.filter((i) => verdict(scores[i][k], group.expected, t) === true).length,
		);
		const short = (kind: AlignerKindDto) => ALIGNERS.find((a) => a.kind === kind)?.short ?? kind;
		stat.textContent = `${group.label}: ${kinds
			.map((kind, k) => `${kinds.length > 1 ? `${short(kind)} ` : ""}${counts[k]}/${indexes.length}`)
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
	t: number,
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
	outcomes.forEach((outcome, k) => head.append(chip(outcome, kinds[k], expected, t)));

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
	const short = ALIGNERS.find((a) => a.kind === kind)?.short ?? kind;
	if (!outcome.ok) {
		b.textContent = `${short} ⚠ ${outcome.message}`;
		b.classList.add("bad");
		return b;
	}
	const right = verdict(outcome, expected, t);
	const mark = right === null ? "·" : right ? "✓" : "✗";
	b.textContent = `${short} ${mark} ${outcome.dto.similarity.toFixed(3)}`;
	if (right !== null) b.classList.add(right ? "ok" : "bad");
	b.title = `${kind}: raw ${outcome.dto.raw} / self ${outcome.dto.self_a}, ${outcome.dto.self_b}`;
	return b;
}

// One aligner's alignment: both lines as words of phoneme chips, tinted by
// the segment that aligned them (in place / moved), greyed when unaligned and
// outlined when reused; then each segment's steps, a over b with scores.
function alignmentEl(dto: EchoAlignmentDto): HTMLElement {
	const box = document.createElement("div");
	box.className = "echo-alignment";

	const moved = dto.segments.filter((s) => s.placement === "moved").length;
	const reused =
		dto.a_coverage.reduce((n, c) => n + Math.max(0, c - 1), 0) +
		dto.b_coverage.reduce((n, c) => n + Math.max(0, c - 1), 0);
	const title = document.createElement("div");
	title.className = "echo-title";
	title.textContent =
		`${dto.aligner} · similarity ${dto.similarity.toFixed(3)} · raw ${dto.raw} ` +
		`(self ${dto.self_a} / ${dto.self_b}) · ${dto.segments.length} segment(s), ${moved} moved` +
		`${reused ? `, ${reused} reused` : ""} · ${dto.variant_pairs_tried} variant pair(s)`;
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
		lineEl(dto.a_words, dto.a_phonemes, dto.a_word_starts, dto.a_coverage, segmentOf("a"), dto),
		lineEl(dto.b_words, dto.b_phonemes, dto.b_word_starts, dto.b_coverage, segmentOf("b"), dto),
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
	coverage: number[],
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
			if (coverage[i] === 0 || segment === undefined) {
				ph.classList.add("unaligned");
			} else {
				ph.classList.add(dto.segments[segment].placement);
				ph.dataset.segment = String(segment + 1);
				ph.title = `segment ${segment + 1} (${dto.segments[segment].placement.replace("_", " ")})`;
			}
			if (coverage[i] > 1) {
				ph.classList.add("reused");
				ph.title += ` · used ${coverage[i]}×`;
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
