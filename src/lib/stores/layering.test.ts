// The `src/lib` layering rule: stores sit below the shell and the ui kit, so nothing
// under `src/lib/stores` may import from `$lib/shell` or `$lib/ui`. `persist` and
// `toasts` live under `$lib/platform` for that reason (ARCH-17). The one allowance is a
// type-only import from `$lib/shell/views`, which holds the `Tab` type and no component.

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const STORES = join(__dirname);

/** Every `from '...'` specifier in a file, with the `import type` ones flagged. */
function imports(source: string): { spec: string; typeOnly: boolean }[] {
	const out: { spec: string; typeOnly: boolean }[] = [];
	const re = /import\s+(type\s+)?[^;]*?\bfrom\s+'([^']+)'/g;
	for (const m of source.matchAll(re)) out.push({ spec: m[2], typeOnly: !!m[1] });
	return out;
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
});
