// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';

import { applyThemePack, clearThemePack, isCssColor, isCssLength, themeTokenAllowlist, validateThemePack } from './theme-pack';

describe('themeTokenAllowlist', () => {
	const allowed = themeTokenAllowlist();

	it('includes colour tokens declared in colors.css', () => {
		for (const name of ['--bg-base', '--bg-surface', '--accent', '--accent-muted', '--text-primary', '--border-strong']) {
			expect(allowed.has(name), name).toBe(true);
		}
	});

	it('includes the two radius tokens', () => {
		expect(allowed.has('--radius-sm')).toBe(true);
		expect(allowed.has('--radius-md')).toBe(true);
	});

	it('excludes aliases, shadows and radius-lg', () => {
		expect(allowed.has('--surface-window')).toBe(false);
		expect(allowed.has('--shadow-sm')).toBe(false);
		expect(allowed.has('--radius-lg')).toBe(false);
	});
});

describe('isCssColor', () => {
	it('accepts hex and function forms', () => {
		for (const good of ['#fff', '#ffff', '#4C8DF6', '#4C8DF6AA', 'rgb(1,2,3)', 'rgba(1,2,3,0.5)', 'hsl(1,2%,3%)', 'oklch(0.5 0.1 200)']) {
			expect(isCssColor(good), good).toBe(true);
		}
	});

	it('rejects non-colours', () => {
		for (const bad of ['red', '3px', '#gggggg', '#12345', 'var(--bg-base)', '']) {
			expect(isCssColor(bad), bad).toBe(false);
		}
	});
});

describe('isCssLength', () => {
	it('accepts plain lengths', () => {
		for (const good of ['0', '3px', '0.5rem', '12em', '50%']) {
			expect(isCssLength(good), good).toBe(true);
		}
	});

	it('rejects non-lengths', () => {
		for (const bad of ['px', '3', '3xy', '-3px-', '']) {
			expect(isCssLength(bad), bad).toBe(false);
		}
	});
});

describe('validateThemePack', () => {
	it('accepts a well-formed pack', () => {
		const pack = validateThemePack({ name: 'Ocean', base: 'light', tokens: { '--accent': '#2563EB', '--radius-md': '6px' } });
		expect(pack).toEqual({ name: 'Ocean', base: 'light', tokens: { '--accent': '#2563EB', '--radius-md': '6px' } });
	});

	it('rejects a non-object', () => {
		expect(() => validateThemePack('nope')).toThrow();
		expect(() => validateThemePack(null)).toThrow();
	});

	it('rejects a missing or blank name', () => {
		expect(() => validateThemePack({ base: 'dark', tokens: {} })).toThrow(/name/);
		expect(() => validateThemePack({ name: '  ', base: 'dark', tokens: {} })).toThrow(/name/);
	});

	it('rejects a base other than dark or light', () => {
		expect(() => validateThemePack({ name: 'x', base: 'purple', tokens: {} })).toThrow(/base/);
	});

	it('rejects a missing tokens object', () => {
		expect(() => validateThemePack({ name: 'x', base: 'dark' })).toThrow(/tokens/);
	});

	it('rejects an unknown token', () => {
		expect(() => validateThemePack({ name: 'x', base: 'dark', tokens: { '--not-a-token': '#000' } })).toThrow(/Unknown theme token/);
	});

	it('rejects a bad colour value', () => {
		expect(() => validateThemePack({ name: 'x', base: 'dark', tokens: { '--accent': 'not-a-colour' } })).toThrow(/colour/);
	});

	it('rejects a bad radius value', () => {
		expect(() => validateThemePack({ name: 'x', base: 'dark', tokens: { '--radius-sm': 'not-a-length' } })).toThrow(/length/);
	});

	it('rejects radius-lg, which is not one of the two offered', () => {
		expect(() => validateThemePack({ name: 'x', base: 'dark', tokens: { '--radius-lg': '3px' } })).toThrow(/Unknown theme token/);
	});
});

describe('applyThemePack / clearThemePack', () => {
	beforeEach(() => {
		clearThemePack();
	});

	it('sets each token as an inline custom property on documentElement', () => {
		applyThemePack({ name: 'Ocean', base: 'dark', tokens: { '--accent': '#2563EB', '--radius-sm': '4px' } });
		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#2563EB');
		expect(document.documentElement.style.getPropertyValue('--radius-sm')).toBe('4px');
	});

	it('removes the previous pack tokens a new one drops', () => {
		applyThemePack({ name: 'Ocean', base: 'dark', tokens: { '--accent': '#2563EB', '--radius-sm': '4px' } });
		applyThemePack({ name: 'Forest', base: 'dark', tokens: { '--accent': '#1A9E63' } });
		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#1A9E63');
		expect(document.documentElement.style.getPropertyValue('--radius-sm')).toBe('');
	});

	it('clearThemePack removes every applied token', () => {
		applyThemePack({ name: 'Ocean', base: 'dark', tokens: { '--accent': '#2563EB' } });
		clearThemePack();
		expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
	});
});
