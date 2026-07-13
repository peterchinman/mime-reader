import init, {
	Analyzer,
	type RhymeAlignmentStepDto,
	type RhymeConfigDto,
	type RhymeDto,
} from "mime_reader";
import rhymePairTests from "../../../rhyme_pair_tests.json";

// The dictionary is served as a static asset (see web/tuning/public/, which
// symlinks the repo's data/CMUdict/cmudict-0.7b).
const DICT_URL = "/cmudict-0.7b";

// Tunable rhyme-distance config fields (all non-negative integers).
const CONFIG_FIELDS: Array<keyof RhymeConfigDto> = [
	"vowel_to_consonant_mismatch",
	"vowel_coefficient",
	"consonant_coefficient",
	"vowel_indel_penalty",
	"consonant_indel_penalty",
	"consonant_repeated_penalty",
	"transposition_cost",
];

// Copied verbatim from the doc comments on `RhymeDistanceConfig`
// (src/rhyme_distance.rs). Keep in sync if those change — tsify doesn't carry
// Rust field docs across the wasm boundary, so this can't be derived at runtime.
// TODO paragraphs are stripped for display; see `description`.
const CONFIG_DOCS: Record<keyof RhymeConfigDto, string> = {
	vowel_to_consonant_mismatch:
		"We never want to compare a vowel to a consonant, so we should give this an arbitrarily high penalty.",
	vowel_coefficient:
		"Vowel distance ranges from 0-9 (VowelHexGraph distance (<= 4), gets multiplied by VOWEL_DISTANCE_COEFFICIENT (2), and VOWEL_STRESS_PENALTY (1) added), consonant distance range up to 10.\n\n" +
		'This coefficient determines "how much more important vowels are than consonants"\n\n' +
		"TODO find the correct value for this. My instinct is that vowels should be significantly more important than consonants.",
	consonant_coefficient:
		"This coefficient determines how much consonant differences contribute to rhyme distance.",
	vowel_indel_penalty:
		"Ideally, we should be comparing equal syllable slices, in which case, the insertion/deletion penalty for vowels should be arbitrarily high, as we always want the vowels to align.\n\n" +
		"TODO: allow comparing different sized syllable slices? (in which case I think what you want to do is... find the smaller-sized slice of the larger one with the lowest distance score)",
	consonant_indel_penalty:
		"Insertion/deletion penalty for consonants\n\n" +
		"This should be greater or equal to half of the default unrelated consonant penalty, otherwise it's cheaper to insert/delete a consonant than to compare unrelated consonants.\n\n" +
		'TODO: Should all inserted consonants have the same insertion penalty? Are some consonants "lighter" than others?',
	consonant_repeated_penalty:
		'"Reapeated" consonants should be penalized less than standard INDEL penalty, for example when comparing TEN DRY <=> TEND DRY (bad example, you get the idea)\n\n' +
		"TODO: an /R/ following /ER/ should be counted as repeated\n\n" +
		"TODO: a /W/ following /OW/ should be counted as repeated\n\n" +
		"TODO: are there other vowel -> consonant sequences that should count as repeated?\n\n" +
		"TODO: This should take into account cases like RAISING (R EY1 Z IH0 NG) vs RAISE SING (R EY1 Z S IH1 NG), i.e. we should take consonant distance of repeated consonants into account\n\n" +
		"An arbitrary starting point would probably be (1 + distance())/2",
	transposition_cost:
		"Transposing consonants should incur a smaller penalty than comparing them each in order. E.g. MOST <=> MOATS should have relatively small penalty.\n\n" +
		"TODO: What should this value be?",
};

const $ = <T extends HTMLElement>(sel: string): T =>
	document.querySelector(sel) as T;

// The doc strings are kept verbatim (see CONFIG_DOCS), but the developer TODO
// notes aren't useful while tuning, so drop those paragraphs for display.
function description(field: keyof RhymeConfigDto): string {
	return CONFIG_DOCS[field]
		.split("\n\n")
		.filter((para) => !para.trimStart().startsWith("TODO"))
		.join("\n\n");
}

interface Pair {
	leader: string;
	follower: string;
	group?: PairGroup;
}

type PairTuple = [string, string];
type PairGroup = string;
type ThresholdPresetGroup = "further_slant_rhymes";

interface FixtureGroup {
	key: PairGroup;
	label: string;
	pairs: PairTuple[];
}

const FIXTURE_GROUPS = fixtureGroupsFrom(rhymePairTests);
const THRESHOLD_PRESETS: Array<{
	group: ThresholdPresetGroup;
	buttonId: string;
	label: string;
}> = [
	{
		group: "further_slant_rhymes",
		buttonId: "#threshold-further",
		label: "further_max",
	},
];

function fixturePair([leader, follower]: PairTuple, group: PairGroup): Pair {
	return { leader, follower, group };
}

function fixtureGroupsFrom(data: unknown): FixtureGroup[] {
	if (!data || typeof data !== "object" || Array.isArray(data)) return [];

	return Object.entries(data).flatMap(([key, value]) => {
		if (!Array.isArray(value)) return [];

		const pairs = value.filter(isPairTuple);
		return pairs.length === 0 ? [] : [{ key, label: key, pairs }];
	});
}

function isPairTuple(value: unknown): value is PairTuple {
	return (
		Array.isArray(value) &&
		value.length === 2 &&
		value.every((item) => typeof item === "string")
	);
}

let analyzer: Analyzer;
let config: RhymeConfigDto;
let activeThresholdPreset: ThresholdPresetGroup | null = null;
const savedPairs: Pair[] = [
	...FIXTURE_GROUPS.flatMap(({ key, pairs }) =>
		pairs.map((pair) => fixturePair(pair, key)),
	),
];

async function main(): Promise<void> {
	await init();

	const res = await fetch(DICT_URL);
	if (!res.ok)
		throw new Error(`failed to fetch dictionary (HTTP ${res.status})`);
	analyzer = new Analyzer(new Uint8Array(await res.arrayBuffer()));
	config = analyzer.rhymeConfig();

	buildConfigControls();
	for (const sel of ["#leader", "#follower"]) {
		$(sel).addEventListener("input", render);
	}
	$<HTMLInputElement>("#threshold").addEventListener("input", () => {
		activeThresholdPreset = null;
		syncThresholdPresetButtons();
		render();
	});
	$<HTMLButtonElement>("#save").addEventListener("click", savePair);
	for (const { group, buttonId } of THRESHOLD_PRESETS) {
		$<HTMLButtonElement>(buttonId).addEventListener("click", () =>
			toggleThresholdPreset(group),
		);
	}
	syncThresholdPresetButtons();

	$("#status").hidden = true;
	$("main").hidden = false;
	render();
}

function currentPair(): Pair {
	return {
		leader: $<HTMLInputElement>("#leader").value.trim(),
		follower: $<HTMLInputElement>("#follower").value.trim(),
	};
}

function threshold(): number {
	const rawValue = $<HTMLInputElement>("#threshold").value.trim();
	if (rawValue === "") return 0.5;
	const value = Number(rawValue);
	return Number.isFinite(value) ? value : 0.5;
}

function maxNormalizedDistance(group: ThresholdPresetGroup): number | null {
	const distances = savedPairs
		.filter((pair) => pair.group === group)
		.map((pair) => evaluatePair(pair, threshold()).rhyme)
		.flatMap((rhyme) =>
			rhyme.status === "follower" ? [rhyme.distance.normalized_distance] : [],
		);

	return distances.length === 0 ? null : Math.max(...distances);
}

function toggleThresholdPreset(group: ThresholdPresetGroup): void {
	activeThresholdPreset = activeThresholdPreset === group ? null : group;
	applyActiveThresholdPreset();
	syncThresholdPresetButtons();
	render();
}

function applyActiveThresholdPreset(): void {
	if (!activeThresholdPreset) return;

	const maxDistance = maxNormalizedDistance(activeThresholdPreset);
	if (maxDistance === null) {
		activeThresholdPreset = null;
		return;
	}

	$<HTMLInputElement>("#threshold").value = formatThreshold(maxDistance);
}

function syncThresholdPresetButtons(): void {
	for (const { group, buttonId, label } of THRESHOLD_PRESETS) {
		const button = $<HTMLButtonElement>(buttonId);
		const active = activeThresholdPreset === group;
		button.classList.toggle("active", active);
		button.setAttribute("aria-pressed", String(active));
		button.disabled = !savedPairs.some((pair) => pair.group === group);
		button.textContent = `${label}: ${active ? "ON" : "OFF"}`;
	}
}

function formatThreshold(value: number): string {
	return (Math.ceil(value * 100) / 100).toFixed(2);
}

function savePair(): void {
	const pair = currentPair();
	if (!pair.leader || !pair.follower) return; // nothing complete to save
	savedPairs.unshift(pair);
	$<HTMLInputElement>("#leader").value = "";
	$<HTMLInputElement>("#follower").value = "";
	$<HTMLInputElement>("#leader").focus();
	render();
}

function render(): void {
	const t = threshold();
	const out = $("#output");
	out.replaceChildren();

	// The live pair (whatever's in the inputs) shows first, then user-saved pairs,
	// then the pre-populated fixture groups.
	const current = currentPair();
	if (current.leader && current.follower) {
		out.append(pairRow(current, t, null));
	}
	appendPairGroup(
		out,
		"saved",
		savedPairsWithIndexes((pair) => !pair.group),
		t,
	);
	for (const { key, label } of FIXTURE_GROUPS) {
		appendPairGroup(
			out,
			label,
			savedPairsWithIndexes((pair) => pair.group === key),
			t,
		);
	}
}

// `index` is the savedPairs index, or null for the live pair from the inputs.
function pairRow(pair: Pair, t: number, index: number | null): HTMLLIElement {
	const li = document.createElement("li");
	li.className = index === null ? "pair current" : "pair";

	const words = document.createElement("span");
	words.className = "pair-words";
	words.textContent = `${pair.leader} → ${pair.follower}`;

	const { rhyme, alignment } = evaluatePair(pair, t);
	li.append(words, rhymeBadge(rhyme));
	if (alignment) {
		li.append(alignmentEl(alignment));
	}

	if (index !== null) {
		const remove = document.createElement("button");
		remove.type = "button";
		remove.className = "remove";
		remove.textContent = "✕";
		remove.title = "remove pair";
		remove.addEventListener("click", () => {
			savedPairs.splice(index, 1);
			applyActiveThresholdPreset();
			syncThresholdPresetButtons();
			render();
		});
		li.append(remove);
	}
	return li;
}

function savedPairsWithIndexes(
	predicate: (pair: Pair) => boolean,
): Array<[Pair, number]> {
	return savedPairs
		.map((pair, index): [Pair, number] => [pair, index])
		.filter(([pair]) => predicate(pair));
}

function appendPairGroup(
	out: HTMLElement,
	label: string,
	pairs: Array<[Pair, number]>,
	t: number,
): void {
	if (pairs.length === 0) return;

	const heading = document.createElement("li");
	heading.className = "pair-group-heading";
	heading.textContent = `${label} (${pairs.length})`;
	out.append(heading);

	for (const [pair, index] of pairs) {
		out.append(pairRow(pair, t, index));
	}
}

interface Evaluation {
	rhyme: RhymeDto;
	// Diagnostic alignment detail; null unless the line is a scored follower.
	alignment: RhymeAlignmentStepDto[] | null;
}

// Evaluate one leader/follower pair under the current config. A two-line "AA"
// poem makes the follower rhyme against the leader; reading line 1 gives the
// leader→follower distance. Re-run every render, so saved pairs stay live.
function evaluatePair(pair: Pair, t: number): Evaluation {
	const poem = analyzer.openPoem(
		[pair.leader, pair.follower],
		undefined,
		"AA",
		t,
	);
	try {
		const rhyme = poem.rhymeAnalysis(1);
		const alignment =
			rhyme.status === "follower" ? (poem.rhymeAlignment(1) ?? null) : null;
		return { rhyme, alignment };
	} finally {
		poem.free(); // the returned values are owned JS, safe to use after
	}
}

function rhymeBadge(rhyme: RhymeDto): HTMLElement {
	const b = document.createElement("span");
	b.className = "badge";
	// TypeScript narrows each branch from the `status` discriminant:
	switch (rhyme.status) {
		case "follower":
			// The compared rhyming parts themselves are rendered by alignmentEl.
			b.textContent = `${rhyme.passed ? "✓" : "✗"} ${rhyme.distance.normalized_distance.toFixed(
				2,
			)} · raw ${rhyme.distance.raw_distance}`;
			b.classList.add(rhyme.passed ? "ok" : "bad");
			break;
		case "error":
			b.textContent = `⚠ ${rhyme.message}`;
			b.classList.add("bad");
			break;
		default:
			// leader / unconstrained / not_applicable can't occur for line 1 of "AA".
			b.textContent = rhyme.status;
			break;
	}
	return b;
}

// The step-by-step alignment the score came from: one column per step, leader
// phoneme on top, target phoneme in the middle, and the step's cost below,
// color-coded by step kind (see the legend in index.html). Gaps render as "–".
function alignmentEl(alignment: RhymeAlignmentStepDto[]): HTMLElement {
	const strip = document.createElement("span");
	strip.className = "align";
	alignment.forEach(({ step, cost }) => {
		const col = document.createElement("span");
		col.className = `col ${step.kind}`;

		const top = document.createElement("span");
		const bottom = document.createElement("span");
		if (step.kind === "indel") {
			top.textContent = step.a ?? "–";
			bottom.textContent = step.b ?? "–";
			col.title = step.a ? "indel: only in leader" : "indel: only in target";
		} else {
			top.textContent = step.a;
			bottom.textContent = step.b;
			col.title =
				step.kind === "transposition"
					? `transposition (matches to the ${step.direction})`
					: step.kind;
		}

		const costEl = document.createElement("span");
		costEl.className = "cost";
		costEl.textContent = String(cost);

		col.append(top, bottom, costEl);
		strip.append(col);
	});
	return strip;
}

function buildConfigControls(): void {
	const container = $("#config-controls");
	for (const field of CONFIG_FIELDS) {
		const row = document.createElement("label");
		row.className = "config-row";

		const name = document.createElement("span");
		name.textContent = field;

		const input = document.createElement("input");
		input.type = "number";
		input.min = "0";
		input.step = "1";
		input.value = String(config[field]);

		input.addEventListener("input", () => {
			if (input.value === "") return; // mid-edit; leave config untouched
			const n = Number(input.value);
			if (!Number.isFinite(n) || n < 0) return;
			config[field] = n; // mutate the plain JS config object
			analyzer.setRhymeConfig(config); // reload the scorer with the new config
			applyActiveThresholdPreset();
			syncThresholdPresetButtons();
			render(); // re-evaluate every pair (live + saved) under the new config
		});

		const doc = document.createElement("small");
		doc.className = "config-doc";
		doc.textContent = description(field);

		row.append(name, input, doc);
		container.append(row);
	}
}

main().catch((err) => {
	$("#status").textContent = `Error: ${err}`;
	$("#status").hidden = false;
});
