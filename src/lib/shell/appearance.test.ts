// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ setSettings: vi.fn().mockResolvedValue({}) })
}));

import { applyAppearance, FONT_MONO_KEY, FONT_SIZE_KEY, FONT_UI_KEY, THEME_PACK_KEY } from './appearance';
import { THEME_KEY, shell } from './shell.svelte';

describe('applyAppearance', () => {
	beforeEach(() => {
		localStorage.clear();
		shell.theme = 'dark';
	});

	it('paints the document and mirrors every value into localStorage', () => {
		applyAppearance('light', null, 'inter', 'jetbrains-mono', 13);

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
		applyAppearance('dark', pack, 'system', 'system-mono', 12);

		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#2563EB');
		expect(JSON.parse(localStorage.getItem(THEME_PACK_KEY)!)).toEqual(pack);
	});

	it('clears a stored pack when switching back to a plain theme', () => {
		const pack = { name: 'Ocean', base: 'dark' as const, tokens: { '--accent': '#2563EB' } };
		applyAppearance('dark', pack, 'system', 'jetbrains-mono', 12);
		applyAppearance('dark', null, 'system', 'jetbrains-mono', 12);

		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
		expect(localStorage.getItem(THEME_PACK_KEY)).toBeNull();
	});
});
