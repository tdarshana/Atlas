import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const repo = fileURLToPath(new URL('../../..', import.meta.url));
const bundle =
	repo +
	'references/Atlas Desktop UI Design File/_ds/' +
	'dbman-design-system-c4e4e5f5-d936-46db-9e21-75bddd719275/_ds_bundle.js';

describe('extract-ds-css', () => {
	it('slices every listed component out of the design system bundle', () => {
		execFileSync('node', [repo + 'scripts/extract-ds-css.mjs', bundle], { stdio: 'pipe' });

		const css = readFileSync(repo + 'src/lib/ds/dbman.css', 'utf8');
		expect(css).toContain('.dbm-btn{');
		expect(css).toContain('.dbm-rail{');
		expect(css).toContain('scripts/extract-ds-css.mjs');
	});
});
