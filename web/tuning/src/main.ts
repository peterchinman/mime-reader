import init, {
	Analyzer,
	defaultRhymeComparisonConfig,
	type ConsonantConfigDto,
	type RhymeAlignmentStepDto,
	type RhymeComparisonConfigDto,
	type RhymeConfigDto,
	type RhymeDto,
	type RhymeNormalizationDto,
	type VowelConfigDto,
} from "mime_reader";
import rhymePairTests from "../../../rhyme_pair_tests.json";

// The dictionary is served as a static asset (see web/tuning/public/, which
// symlinks the repo's data/CMUdict/cmudict-0.7b).
const DICT_URL = "/cmudict-0.7b";
const CONFIG_STORAGE_KEY = "mime_reader:tuning:distance-configs:v1";
const U32_MAX = 4_294_967_295;

type ConfigKind = "vowel" | "consonant" | "rhyme";

const CONFIG_SECTIONS: Array<{
	key: ConfigKind;
	title: string;
}> = [
	{ key: "vowel", title: "vowel distance config" },
	{ key: "consonant", title: "consonant distance config" },
	{ key: "rhyme", title: "rhyme distance config" },
];

const VOWEL_CONFIG_FIELDS: Array<keyof VowelConfigDto> = [
	"vowel_stress_penalty",
	"vowel_distance_coefficient",
	"er_ah_distance",
];

const CONSONANT_CONFIG_FIELDS: Array<keyof ConsonantConfigDto> = [
	"place_distance_coefficient",
	"unrelated_penalty",
	"voiced_penalty",
	"sibilance_penalty",
	"r_l_distance",
	"w_v_distance",
	"affricate_sibilant_fricative_penalty",
	"affricate_plosive_penalty",
	"affricate_non_sibilant_fricative_penalty",
];

const RHYME_CONFIG_FIELDS: Array<keyof RhymeConfigDto> = [
	"vowel_to_consonant_mismatch",
	"vowel_coefficient",
	"consonant_coefficient",
	"vowel_indel_penalty",
	"consonant_indel_penalty",
	"consonant_repeated_penalty",
	"transposition_cost",
];

const VOWEL_CONFIG_DOCS: Record<keyof VowelConfigDto, string> = {
	vowel_stress_penalty:
		"Penalty applied when two vowels have different stress levels.",
	vowel_distance_coefficient:
		"How much to weight raw vowel phoneme distance relative to the vowel stress penalty.",
	er_ah_distance:
		"Raw graph edge distance between ER in BIRD and AH in BUT, before the vowel distance coefficient.",
};

const CONSONANT_CONFIG_DOCS: Record<keyof ConsonantConfigDto, string> = {
	place_distance_coefficient:
		"Multiplier applied to the raw articulatory place difference.",
	unrelated_penalty:
		"Furthest related consonants are 7 apart, so unrelated consonants should be somewhat further than that.",
	voiced_penalty: "Distance between consonants that differ only in voicing, such as TIN and DIN.",
	sibilance_penalty:
		"Additional distance between same-manner consonants that differ in sibilance, such as SICK and THICK.",
	r_l_distance: "Special-case distance between R and L, such as ROOT and LOOT.",
	w_v_distance: "Special-case distance between W and V, such as WHILE and VIAL.",
	affricate_sibilant_fricative_penalty:
		"Manner penalty between an affricate and a sibilant fricative, such as CHIN and SHIN.",
	affricate_plosive_penalty:
		"Manner penalty between an affricate and a plosive, such as CHIN and TIN.",
	affricate_non_sibilant_fricative_penalty:
		"Manner penalty between an affricate and a non-sibilant fricative, such as CHIN and THIN.",
};

// Copied verbatim from the doc comments on `RhymeDistanceConfig`
// (src/rhyme_distance.rs). Keep in sync if those change — tsify doesn't carry
// Rust field docs across the wasm boundary, so this can't be derived at runtime.
// TODO paragraphs are stripped for display; see `description`.
const CONFIG_DOCS: Record<keyof RhymeConfigDto, string> = {
	vowel_to_consonant_mismatch:
		"We never want to compare a vowel to a consonant, so we should give this an arbitrarily high penalty.",
	vowel_coefficient:
		"Multiplier on normalized vowel distances (0-1000).\n\n" +
		'Together with the consonant coefficient this determines "how much more important vowels are than consonants": the default 8:1 comes from experimentation — a much higher ratio than I would have guessed from the armchair. It is the minimum with which we can differentiate between distant slant rhymes and words that should not rhyme.',
	consonant_coefficient:
		"Multiplier on normalized consonant distances (0-1000): how much consonant differences contribute to rhyme distance.",
	vowel_indel_penalty:
		"Ideally, we should be comparing equal syllable slices, in which case, the insertion/deletion penalty for vowels should be arbitrarily high, as we always want the vowels to align.\n\n" +
		"TODO: allow comparing different sized syllable slices? (in which case I think what you want to do is... find the smaller-sized slice of the larger one with the lowest distance score)",
	consonant_indel_penalty:
		"Insertion/deletion penalty for consonants\n\n" +
		"This should be greater or equal to half the cost of substituting maximally distant consonants (1000 * consonant_coefficient), otherwise it's cheaper to insert/delete a consonant than to compare unrelated consonants.\n\n" +
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
function description(doc: string): string {
	return doc
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

interface StoredConfigState {
	version: 1;
	vowel: unknown;
	consonant: unknown;
	rhyme: unknown;
	normalization: unknown;
	thresholdPreset: unknown;
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

	return Object.entries(data).flatMap(([key, section]) => {
		if (!section || typeof section !== "object" || Array.isArray(section)) {
			return [];
		}

		const pairsValue = (section as Record<string, unknown>).pairs;
		if (!Array.isArray(pairsValue)) return [];

		const pairs = pairsValue.filter(isPairTuple);
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
let defaultVowelConfig: VowelConfigDto;
let defaultConsonantConfig: ConsonantConfigDto;
let defaultRhymeConfig: RhymeConfigDto;
let defaultRhymeNormalization: RhymeNormalizationDto;
let vowelConfig: VowelConfigDto;
let consonantConfig: ConsonantConfigDto;
let rhymeConfig: RhymeConfigDto;
let rhymeNormalization: RhymeNormalizationDto;
let activeConfigKind: ConfigKind = "rhyme";
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
	defaultVowelConfig = analyzer.vowelConfig();
	defaultConsonantConfig = analyzer.consonantConfig();
	defaultRhymeConfig = analyzer.rhymeConfig();
	defaultRhymeNormalization = defaultRhymeComparisonConfig().normalization;

	const stored = loadStoredConfigState();
	vowelConfig = restoreConfig(
		defaultVowelConfig,
		VOWEL_CONFIG_FIELDS,
		stored?.vowel,
	);
	consonantConfig = restoreConfig(
		defaultConsonantConfig,
		CONSONANT_CONFIG_FIELDS,
		stored?.consonant,
	);
	rhymeConfig = restoreConfig(
		defaultRhymeConfig,
		RHYME_CONFIG_FIELDS,
		stored?.rhyme,
	);
	rhymeNormalization = isRhymeNormalization(stored?.normalization)
		? stored.normalization
		: defaultRhymeNormalization;
	activeThresholdPreset = isThresholdPresetGroup(stored?.thresholdPreset)
		? stored.thresholdPreset
		: null;
	applyConfigsToAnalyzer();
	applyActiveThresholdPreset();
	persistConfigs();

	buildConfigSwitcher();
	buildConfigControls();
	buildNormalizationControl();
	$<HTMLButtonElement>("#restore-defaults").addEventListener(
		"click",
		restoreCodebaseDefaults,
	);
	syncRestoreDefaultsButton();
	for (const sel of ["#leader", "#follower"]) {
		$(sel).addEventListener("input", render);
	}
	$<HTMLInputElement>("#threshold").addEventListener("input", () => {
		activeThresholdPreset = null;
		persistConfigs();
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

function isRecord(value: unknown): value is Record<string, unknown> {
	return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function isConfigValue(value: unknown): value is number {
	return (
		typeof value === "number" &&
		Number.isInteger(value) &&
		value >= 0 &&
		value <= U32_MAX
	);
}

function isRhymeNormalization(value: unknown): value is RhymeNormalizationDto {
	return (
		value === "none" ||
		value === "per_syllable" ||
		value === "per_mean_phoneme"
	);
}

function isThresholdPresetGroup(value: unknown): value is ThresholdPresetGroup {
	return THRESHOLD_PRESETS.some(({ group }) => group === value);
}

function loadStoredConfigState(): StoredConfigState | null {
	try {
		const raw = window.localStorage.getItem(CONFIG_STORAGE_KEY);
		if (!raw) return null;

		const parsed: unknown = JSON.parse(raw);
		if (!isRecord(parsed) || parsed.version !== 1) return null;

		return {
			version: 1,
			vowel: parsed.vowel,
			consonant: parsed.consonant,
			rhyme: parsed.rhyme,
			normalization: parsed.normalization,
			thresholdPreset: parsed.thresholdPreset,
		};
	} catch {
		return null;
	}
}

function restoreConfig<T extends object>(
	defaults: T,
	fields: Array<keyof T>,
	stored: unknown,
): T {
	const restored = { ...defaults };
	if (!isRecord(stored)) return restored;

	const target = restored as Record<string, unknown>;
	for (const field of fields) {
		const value = stored[String(field)];
		if (isConfigValue(value)) target[String(field)] = value;
	}
	return restored;
}

function configIsModified<T, K extends keyof T>(
	config: T,
	defaults: T,
	fields: K[],
): boolean {
	return fields.some((field) => config[field] !== defaults[field]);
}

function configsAreModified(): boolean {
	return (
		configIsModified(vowelConfig, defaultVowelConfig, VOWEL_CONFIG_FIELDS) ||
		configIsModified(
			consonantConfig,
			defaultConsonantConfig,
			CONSONANT_CONFIG_FIELDS,
		) ||
		configIsModified(rhymeConfig, defaultRhymeConfig, RHYME_CONFIG_FIELDS) ||
		rhymeNormalization !== defaultRhymeNormalization
	);
}

function configOverrides<T, K extends keyof T>(
	config: T,
	defaults: T,
	fields: K[],
): Record<string, number> {
	const overrides: Record<string, number> = {};
	for (const field of fields) {
		if (config[field] !== defaults[field]) {
			overrides[String(field)] = config[field] as number;
		}
	}
	return overrides;
}

function persistConfigs(): void {
	try {
		if (!configsAreModified() && activeThresholdPreset === null) {
			window.localStorage.removeItem(CONFIG_STORAGE_KEY);
			return;
		}

		window.localStorage.setItem(
			CONFIG_STORAGE_KEY,
			JSON.stringify({
				version: 1,
				vowel: configOverrides(
					vowelConfig,
					defaultVowelConfig,
					VOWEL_CONFIG_FIELDS,
				),
				consonant: configOverrides(
					consonantConfig,
					defaultConsonantConfig,
					CONSONANT_CONFIG_FIELDS,
				),
				rhyme: configOverrides(
					rhymeConfig,
					defaultRhymeConfig,
					RHYME_CONFIG_FIELDS,
				),
				normalization:
					rhymeNormalization === defaultRhymeNormalization
						? undefined
						: rhymeNormalization,
				thresholdPreset: activeThresholdPreset ?? undefined,
			}),
		);
	} catch {
		// Storage may be unavailable in private or restricted browser contexts.
	}
}

function applyConfigsToAnalyzer(): void {
	analyzer.setVowelConfig(vowelConfig);
	analyzer.setConsonantConfig(consonantConfig);
	analyzer.setRhymeConfig(rhymeConfig);
}

function restoreCodebaseDefaults(): void {
	vowelConfig = { ...defaultVowelConfig };
	consonantConfig = { ...defaultConsonantConfig };
	rhymeConfig = { ...defaultRhymeConfig };
	rhymeNormalization = defaultRhymeNormalization;
	applyConfigsToAnalyzer();
	persistConfigs();
	buildConfigControls();
	syncNormalizationControl();
	syncRestoreDefaultsButton();
	applyActiveThresholdPreset();
	syncThresholdPresetButtons();
	render();
}

function syncRestoreDefaultsButton(): void {
	$<HTMLButtonElement>("#restore-defaults").disabled = !configsAreModified();
}

function buildNormalizationControl(): void {
	const select = $<HTMLSelectElement>("#normalization");
	select.addEventListener("change", () => {
		if (!isRhymeNormalization(select.value)) return;
		rhymeNormalization = select.value;
		persistConfigs();
		syncNormalizationControl();
		syncRestoreDefaultsButton();
		applyActiveThresholdPreset();
		syncThresholdPresetButtons();
		render();
	});
	syncNormalizationControl();
}

function syncNormalizationControl(): void {
	const modified = rhymeNormalization !== defaultRhymeNormalization;
	$<HTMLSelectElement>("#normalization").value = rhymeNormalization;
	$("#normalization-control").classList.toggle("modified", modified);
	$("#normalization-modified").hidden = !modified;
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
	persistConfigs();
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
	const savedEvaluations = savedPairs.map((pair) => evaluatePair(pair, t));
	renderSectionOverview(savedEvaluations);

	// The live pair (whatever's in the inputs) shows first, then user-saved pairs,
	// then the pre-populated fixture groups.
	const current = currentPair();
	if (current.leader && current.follower) {
		out.append(pairRow(current, null, evaluatePair(current, t)));
	}
	appendPairGroup(
		out,
		"saved",
		savedPairsWithIndexes((pair) => !pair.group),
		savedEvaluations,
	);
	for (const { key, label } of FIXTURE_GROUPS) {
		appendPairGroup(
			out,
			label,
			savedPairsWithIndexes((pair) => pair.group === key),
			savedEvaluations,
		);
	}
}

function renderSectionOverview(evaluations: Evaluation[]): void {
	const overview = $("#section-overview");
	overview.replaceChildren();

	for (const { key, label } of FIXTURE_GROUPS) {
		const indexes = savedPairs
			.map((pair, index) => (pair.group === key ? index : -1))
			.filter((index) => index >= 0);
		const passed = indexes.filter(
			(index) => evaluations[index]?.rhyme.status === "follower" &&
				evaluations[index].rhyme.passed,
		).length;

		const stat = document.createElement("span");
		stat.className = "section-stat";
		stat.textContent = `${label}: ${passed}/${indexes.length}`;
		if (indexes.length > 0 && passed === indexes.length) {
			stat.classList.add("all-pass");
		} else if (indexes.length > 0 && passed === 0) {
			stat.classList.add("all-fail");
		}
		overview.append(stat);
	}
}

// `index` is the savedPairs index, or null for the live pair from the inputs.
function pairRow(
	pair: Pair,
	index: number | null,
	evaluation: Evaluation,
): HTMLLIElement {
	const li = document.createElement("li");
	li.className = index === null ? "pair current" : "pair";

	const words = document.createElement("span");
	words.className = "pair-words";
	words.textContent = `${pair.leader} → ${pair.follower}`;

	const { rhyme, alignment } = evaluation;
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
			persistConfigs();
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
	evaluations: Evaluation[],
): void {
	if (pairs.length === 0) return;

	const heading = document.createElement("li");
	heading.className = "pair-group-heading";
	heading.textContent = `${label} (${pairs.length})`;
	out.append(heading);

	for (const [pair, index] of pairs) {
		out.append(pairRow(pair, index, evaluations[index]));
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
	const comparisonConfig: RhymeComparisonConfigDto = {
		threshold: t,
		normalization: rhymeNormalization,
	};
	const poem = analyzer.openPoem([pair.leader, pair.follower], {
		rhyme_scheme: "AA",
		rhyme_comparison_config: comparisonConfig,
	});
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
			)} · raw ${formatScore(rhyme.distance.raw_distance)}`;
			b.classList.add(rhyme.passed ? "ok" : "bad");
			b.title = `${rhyme.distance.normalization}; raw ÷ ${rhyme.distance.normalization_denominator}`;
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
		costEl.textContent = formatScore(cost);

		col.append(top, bottom, costEl);
		strip.append(col);
	});
	return strip;
}

function formatScore(value: number): string {
	return Number.isInteger(value)
		? String(value)
		: value.toFixed(2).replace(/0+$/, "").replace(/\.$/, "");
}

function buildConfigSwitcher(): void {
	for (const { key } of CONFIG_SECTIONS) {
		const button = $<HTMLButtonElement>(`#config-${key}`);
		button.addEventListener("click", () => activateConfigKind(key));
		button.addEventListener("keydown", (event) => {
			if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;

			event.preventDefault();
			const index = CONFIG_SECTIONS.findIndex((section) => section.key === key);
			const offset = event.key === "ArrowRight" ? 1 : -1;
			const next = CONFIG_SECTIONS.at(
				(index + offset + CONFIG_SECTIONS.length) % CONFIG_SECTIONS.length,
			);
			if (!next) return;
			activateConfigKind(next.key);
			$<HTMLButtonElement>(`#config-${next.key}`).focus();
		});
	}
	syncConfigSwitcher();
}

function activateConfigKind(kind: ConfigKind): void {
	if (activeConfigKind === kind) return;
	activeConfigKind = kind;
	syncConfigSwitcher();
	buildConfigControls();
}

function syncConfigSwitcher(): void {
	const activeSection = CONFIG_SECTIONS.find(
		(section) => section.key === activeConfigKind,
	);
	if (!activeSection) return;

	for (const { key } of CONFIG_SECTIONS) {
		const button = $<HTMLButtonElement>(`#config-${key}`);
		const active = key === activeConfigKind;
		button.setAttribute("aria-selected", String(active));
		button.tabIndex = active ? 0 : -1;
	}

	$("#config-title").textContent = activeSection.title;
	$("#config-controls").setAttribute(
		"aria-labelledby",
		`config-${activeSection.key}`,
	);
}

function buildConfigControls(): void {
	switch (activeConfigKind) {
		case "vowel":
			renderConfigControls(
				vowelConfig,
				defaultVowelConfig,
				VOWEL_CONFIG_FIELDS,
				VOWEL_CONFIG_DOCS,
				(field, value) => {
					vowelConfig[field] = value;
					analyzer.setVowelConfig(vowelConfig);
				},
			);
			break;
		case "consonant":
			renderConfigControls(
				consonantConfig,
				defaultConsonantConfig,
				CONSONANT_CONFIG_FIELDS,
				CONSONANT_CONFIG_DOCS,
				(field, value) => {
					consonantConfig[field] = value;
					analyzer.setConsonantConfig(consonantConfig);
				},
			);
			break;
		case "rhyme":
			renderConfigControls(
				rhymeConfig,
				defaultRhymeConfig,
				RHYME_CONFIG_FIELDS,
				CONFIG_DOCS,
				(field, value) => {
					rhymeConfig[field] = value;
					analyzer.setRhymeConfig(rhymeConfig);
					},
				);
			break;
	}
}

function renderConfigControls<T, K extends keyof T>(
	config: T,
	defaults: T,
	fields: K[],
	docs: Record<K, string>,
	setValue: (field: K, value: number) => void,
): void {
	const container = $("#config-controls");
	container.replaceChildren();
	for (const field of fields) {
		const row = document.createElement("label");
		row.className = "config-row";

		const name = document.createElement("span");
		name.className = "config-name";
		const fieldName = document.createElement("span");
		fieldName.className = "config-field-name";
		fieldName.textContent = String(field);
		const modifiedMarker = document.createElement("small");
		modifiedMarker.className = "config-modified";
		modifiedMarker.textContent = "changed";
		name.append(fieldName, modifiedMarker);

		const input = document.createElement("input");
		input.type = "number";
		input.min = "0";
		input.max = String(U32_MAX);
		input.step = "1";
		input.value = String(config[field]);
		syncConfigRowModified(
			row,
			modifiedMarker,
			config[field] !== defaults[field],
		);

		input.addEventListener("input", () => {
			if (input.value === "") return; // mid-edit; leave config untouched
			const n = Number(input.value);
			if (!isConfigValue(n)) return;
			setValue(field, n);
			syncConfigRowModified(
				row,
				modifiedMarker,
				config[field] !== defaults[field],
			);
			persistConfigs();
			syncRestoreDefaultsButton();
			applyActiveThresholdPreset();
			syncThresholdPresetButtons();
			render(); // re-evaluate every pair (live + saved) under the new config
		});

		const doc = document.createElement("small");
		doc.className = "config-doc";
		doc.textContent = description(docs[field]);

		row.append(name, input, doc);
		container.append(row);
	}
}

function syncConfigRowModified(
	row: HTMLElement,
	marker: HTMLElement,
	modified: boolean,
): void {
	row.classList.toggle("modified", modified);
	marker.hidden = !modified;
}

main().catch((err) => {
	$("#status").textContent = `Error: ${err}`;
	$("#status").hidden = false;
});
