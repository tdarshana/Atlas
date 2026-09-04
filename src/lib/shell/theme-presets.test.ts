import { describe, expect, it } from 'vitest';
import { validateThemePack } from './theme-pack';
import { THEME_PRESETS, themePreset } from './theme-presets';

describe('theme presets', () => {
	it('ships ten packs, a light and a dark one per palette', () => {
		expect(THEME_PRESETS.length).toBe(10);
		expect(THEME_PRESETS.filter((p) => p.base === 'light').length).toBe(5);
		expect(new Set(THEME_PRESETS.map((p) => p.name)).size).toBe(10);
	});

	it('every preset passes the same validation an imported pack does', () => {
		for (const pack of THEME_PRESETS) {
			expect(() => validateThemePack(JSON.parse(JSON.stringify(pack)))).not.toThrow();
		}
	});

	it('looks a preset up by name', () => {
		expect(themePreset('Vercel dark')?.base).toBe('dark');
		expect(themePreset('nope')).toBeNull();
	});
});
