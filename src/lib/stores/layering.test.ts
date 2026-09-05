// The `src/lib` layering rule: stores sit below the shell and the ui kit, so nothing
// under `src/lib/stores` may import from `$lib/shell` or `$lib/ui`. `persist` and
// `toasts` live under `$lib/platform` for that reason (ARCH-17). The one allowance is a
// type-only import from `$lib/shell/views`, which holds the `Tab` type and no component.
// Components and routes, in turn, write through a store rather than through `$lib/api`.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const STORES = join(__dirname);
const SRC = resolve(__dirname, '..', '..');

/** Every `from '...'` specifier in a file, with the `import type` ones flagged. */
function imports(source: string): { spec: string; typeOnly: boolean }[] {
	const out: { spec: string; typeOnly: boolean }[] = [];
	const re = /import\s+(type\s+)?[^;]*?\bfrom\s+'([^']+)'/g;
	for (const m of source.matchAll(re)) out.push({ spec: m[2], typeOnly: !!m[1] });
	return out;
}

function* walk(dir: string): Generator<string> {
	for (const name of readdirSync(dir)) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* walk(path);
		else if (/\.(ts|js|svelte)$/.test(name)) yield path;
	}
}

describe('store layering', () => {
	it('no store imports from $lib/shell or $lib/ui', () => {
		const offenders: string[] = [];
		for (const name of readdirSync(STORES)) {
			if (!/\.(ts|js)$/.test(name)) continue;
			const source = readFileSync(join(STORES, name), 'utf8');
			for (const { spec, typeOnly } of imports(source)) {
				const shell = spec === '$lib/shell' || spec.startsWith('$lib/shell/');
				const ui = spec === '$lib/ui' || spec.startsWith('$lib/ui/');
				if (!shell && !ui) continue;
				if (typeOnly && spec === '$lib/shell/views') continue;
				offenders.push(`${name}: ${spec}`);
			}
		}
		expect(offenders).toEqual([]);
	});

	// Writes go through a store (ARCH-11): only stores know the api client and its errors,
	// so a component or a route never imports `$lib/api`.
	it('no component or route imports $lib/api', () => {
		const offenders: string[] = [];
		for (const dir of ['lib/components', 'routes']) {
			for (const file of walk(join(SRC, dir))) {
				const source = readFileSync(file, 'utf8');
				if (imports(source).some(({ spec }) => spec === '$lib/api'))
					offenders.push(file.slice(SRC.length + 1));
			}
		}
		expect(offenders).toEqual([]);
	});
});
