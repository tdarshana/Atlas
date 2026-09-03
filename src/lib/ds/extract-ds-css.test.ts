import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';

const repo = fileURLToPath(new URL('../../..', import.meta.url));
const bundle =
	repo +
	'references/Atlas Desktop UI Design File/_ds/' +
	'dbman-design-system-c4e4e5f5-d936-46db-9e21-75bddd719275/_ds_bundle.js';

// The script never writes into the repo here: a stale committed dbman.css has to fail,
// not be quietly repaired by the test run.
const dir = mkdtempSync(join(tmpdir(), 'atlas-ds-'));
const out = join(dir, 'dbman.css');

afterAll(() => rmSync(dir, { recursive: true, force: true }));

describe('extract-ds-css', () => {
	it('slices every listed component out of the design system bundle', () => {
		execFileSync('node', [repo + 'scripts/extract-ds-css.mjs', bundle, out], { stdio: 'pipe' });

		const css = readFileSync(out, 'utf8');
		expect(css).toContain('.dbm-btn{');
		expect(css).toContain('.dbm-rail{');
		expect(css).toContain('scripts/extract-ds-css.mjs');
	});

	it('matches the committed stylesheet', () => {
		execFileSync('node', [repo + 'scripts/extract-ds-css.mjs', bundle, out], { stdio: 'pipe' });

		expect(readFileSync(out, 'utf8')).toBe(readFileSync(repo + 'src/lib/ds/dbman.css', 'utf8'));
	});
});
