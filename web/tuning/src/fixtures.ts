// Fixture files shared by the tuning pages: `{ group: { pairs: [[a, b], ...],
// ...other fields } }` (rhyme_pair_tests.json, echo_pair_tests.json).

export type PairTuple = [string, string];

export interface FixtureGroup {
	key: string;
	label: string;
	pairs: PairTuple[];
	// The group's whole entry, for page-specific fields (comment, class, ...).
	meta: Record<string, unknown>;
}

export function fixtureGroupsFrom(data: unknown): FixtureGroup[] {
	if (!data || typeof data !== "object" || Array.isArray(data)) return [];

	return Object.entries(data).flatMap(([key, section]) => {
		if (!section || typeof section !== "object" || Array.isArray(section)) {
			return [];
		}

		const meta = section as Record<string, unknown>;
		const pairsValue = meta.pairs;
		if (!Array.isArray(pairsValue)) return [];

		const pairs = pairsValue.filter(isPairTuple);
		return pairs.length === 0 ? [] : [{ key, label: key, pairs, meta }];
	});
}

export function isPairTuple(value: unknown): value is PairTuple {
	return (
		Array.isArray(value) &&
		value.length === 2 &&
		value.every((item) => typeof item === "string")
	);
}
