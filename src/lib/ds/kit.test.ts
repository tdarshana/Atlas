// One component kit (ARCH-16): `$lib/ds` is the DBMan port and the design authority, so
// the `ui` copies of Button, Input and Table are gone and nothing under `src` may import
// them. `$lib/ui` keeps only what `ds` has no equivalent for yet (Dialog, Textarea,
// MarkdownView, ResizeBar, EmptyState, ErrorState, Toast).

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = resolve(__dirname, '..', '..');
const RETIRED = ['Button', 'Input', 'Table'];

function* walk(dir: string): Generator<string> {
	for (const name of readdirSync(dir)) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* walk(path);
		else if (/\.(ts|js|svelte)$/.test(name)) yield path;
	}
}

describe('the component kit', () => {
	it('has no ui copy of Button, Input or Table', () => {
		const present = RETIRED.filter((c) => existsSync(join(SRC, 'lib', 'ui', `${c}.svelte`)));
		expect(present).toEqual([]);
	});

	it('nothing under src imports the retired ui components', () => {
		const re = new RegExp(`\\$lib/ui/(${RETIRED.join('|')})\\.svelte`);
		const offenders: string[] = [];
		for (const file of walk(SRC)) {
			if (file.endsWith('kit.test.ts')) continue;
			if (re.test(readFileSync(file, 'utf8'))) offenders.push(file.slice(SRC.length + 1));
		}
		expect(offenders).toEqual([]);
	});
});
