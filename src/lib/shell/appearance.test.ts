// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ setSettings: vi.fn().mockResolvedValue({}) })
}));

import {
	applyAppearance,
	FONT_MONO_KEY,
	FONT_SIZE_KEY,
	FONT_UI_KEY,
	SCALE_KEY,
	SCALE_OPTIONS,
	THEME_PACK_KEY
} from './appearance';
import { THEME_KEY, shell } from './shell.svelte';

describe('applyAppearance', () => {
	beforeEach(() => {
		localStorage.clear();
		shell.theme = 'dark';
		document.documentElement.style.removeProperty('zoom');
	});

	it('paints the document and mirrors every value into localStorage', () => {
		applyAppearance('light', null, 'inter', 'jetbrains-mono', 13, 100);

		expect(shell.theme).toBe('light');
		expect(document.documentElement.dataset.theme).toBe('light');
		expect(document.documentElement.style.getPropertyValue('--font-ui')).toContain('Inter');
		expect(document.documentElement.style.getPropertyValue('--text-sm')).toBe('13px');

		expect(localStorage.getItem(THEME_KEY)).toBe('light');
		expect(localStorage.getItem(THEME_PACK_KEY)).toBeNull();
		expect(localStorage.getItem(FONT_UI_KEY)).toBe('inter');
		expect(localStorage.getItem(FONT_MONO_KEY)).toBe('jetbrains-mono');
		expect(localStorage.getItem(FONT_SIZE_KEY)).toBe('13');
	});

	it('mirrors a theme pack as JSON and applies its tokens', () => {
		const pack = { name: 'Ocean', base: 'dark' as const, tokens: { '--accent': '#2563EB' } };
		applyAppearance('dark', pack, 'system', 'system-mono', 12, 100);

		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#2563EB');
		expect(JSON.parse(localStorage.getItem(THEME_PACK_KEY)!)).toEqual(pack);
	});

	it('clears a stored pack when switching back to a plain theme', () => {
		const pack = { name: 'Ocean', base: 'dark' as const, tokens: { '--accent': '#2563EB' } };
		applyAppearance('dark', pack, 'system', 'jetbrains-mono', 12, 100);
		applyAppearance('dark', null, 'system', 'jetbrains-mono', 12, 100);

		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
		expect(localStorage.getItem(THEME_PACK_KEY)).toBeNull();
	});

	it('applies zoom for a non-100 scale and mirrors atlas.scale', () => {
		applyAppearance('dark', null, 'system', 'jetbrains-mono', 12, 125);

		expect(document.documentElement.style.zoom).toBe('1.25');
		expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe('1.25');
		expect(localStorage.getItem(SCALE_KEY)).toBe('125');
	});

	it('removes zoom and the mirror at 100', () => {
		// jsdom does not recognise `zoom` as a real CSS property, so a value set via the
		// dot assignment never comes back out through `removeProperty`; spy on the call
		// instead of re-reading the (browser-only) style to prove the mechanism runs.
		const removeProperty = vi.spyOn(document.documentElement.style, 'removeProperty');
		applyAppearance('dark', null, 'system', 'jetbrains-mono', 12, 100);

		expect(removeProperty).toHaveBeenCalledWith('zoom');
		expect(removeProperty).toHaveBeenCalledWith('--ui-zoom');
		expect(localStorage.getItem(SCALE_KEY)).toBeNull();
	});
});

describe('SCALE_OPTIONS', () => {
	it('lists the six allowed percents with a % label', () => {
		expect(SCALE_OPTIONS).toEqual([
			{ value: '80', label: '80%' },
			{ value: '90', label: '90%' },
			{ value: '100', label: '100%' },
			{ value: '110', label: '110%' },
			{ value: '125', label: '125%' },
			{ value: '150', label: '150%' }
		]);
	});
});
