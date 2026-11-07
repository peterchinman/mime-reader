// Config editing shared by the tuning pages: numeric config rows with
// "changed" markers, and the helpers that persist only overrides.

export const U32_MAX = 4_294_967_295;

export function isRecord(value: unknown): value is Record<string, unknown> {
	return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function isConfigValue(value: unknown): value is number {
	return (
		typeof value === "number" &&
		Number.isInteger(value) &&
		value >= 0 &&
		value <= U32_MAX
	);
}

// The doc strings are kept verbatim, but the developer TODO notes aren't
// useful while tuning, so drop those paragraphs for display.
export function description(doc: string): string {
	return doc
		.split("\n\n")
		.filter((para) => !para.trimStart().startsWith("TODO"))
		.join("\n\n");
}

export function restoreConfig<T extends object>(
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

export function configIsModified<T, K extends keyof T>(
	config: T,
	defaults: T,
	fields: K[],
): boolean {
	return fields.some((field) => config[field] !== defaults[field]);
}

export function configOverrides<T, K extends keyof T>(
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

// One labeled number input per field, with its doc below and a "changed"
// marker while it differs from the default. `setValue` applies an edit;
// `onChange` runs after every applied edit (persist, re-render, ...).
export function renderConfigControls<T, K extends keyof T>(
	container: HTMLElement,
	config: T,
	defaults: T,
	fields: K[],
	docs: Record<K, string>,
	setValue: (field: K, value: number) => void,
	onChange: () => void,
): void {
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
			onChange();
		});

		const doc = document.createElement("small");
		doc.className = "config-doc";
		doc.textContent = description(docs[field]);

		row.append(name, input, doc);
		container.append(row);
	}
}

export function syncConfigRowModified(
	row: HTMLElement,
	marker: HTMLElement,
	modified: boolean,
): void {
	row.classList.toggle("modified", modified);
	marker.hidden = !modified;
}
