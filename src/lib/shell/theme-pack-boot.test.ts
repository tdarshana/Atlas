// @vitest-environment jsdom
// Task 1 review fix round, finding 1: tokens `src/app.html`'s boot script paints
// before this module loads are never tracked, so switching back to Dark or Light and
// saving does not remove them from `documentElement.style`. Pins that `theme-pack.ts`
// reads the boot script's marker back at startup, so its very first `clearThemePack`
// call already knows about them.

import { afterEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
	document.documentElement.style.cssText = '';
	delete document.documentElement.dataset.themePackTokens;
});

describe('theme-pack module startup', () => {
	it('clearThemePack removes tokens the boot script painted before this module ever loaded', async () => {
		// Simulate what src/app.html's inline script does before the SvelteKit bundle,
		// and this module with it, loads: paint the tokens, then leave their names behind.
		document.documentElement.style.setProperty('--accent', '#2563EB');
		document.documentElement.style.setProperty('--radius-sm', '4px');
		document.documentElement.dataset.themePackTokens = JSON.stringify(['--accent', '--radius-sm']);

		vi.resetModules();
		const { clearThemePack } = await import('./theme-pack');
		clearThemePack();

		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
		expect(document.documentElement.style.getPropertyValue('--radius-sm')).toBe('');
	});

	it('starts with nothing to clear when the boot script left no marker', async () => {
		vi.resetModules();
		const { clearThemePack } = await import('./theme-pack');
		// Nothing was painted, so nothing should be touched; this only pins that a fresh
		// module load without a marker does not throw or misbehave.
		expect(() => clearThemePack()).not.toThrow();
	});

	it('ignores a malformed marker rather than throwing at module load', async () => {
		document.documentElement.dataset.themePackTokens = 'not json';
		vi.resetModules();
		await expect(import('./theme-pack')).resolves.toBeDefined();
	});
});
