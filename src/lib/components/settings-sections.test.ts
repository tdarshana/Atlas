import { describe, expect, it, vi } from 'vitest';
import { SETTINGS_SECTIONS, scrollToSection, sectionFromHash } from './settings-sections';

describe('sectionFromHash', () => {
	it('accepts every id the side panel links to', () => {
		for (const section of SETTINGS_SECTIONS) {
			expect(sectionFromHash(`#${section.id}`)).toBe(section.id);
		}
	});

	it('accepts a bare id as well as a fragment', () => {
		expect(sectionFromHash('extraction')).toBe('extraction');
	});

	it('rejects a hash the page does not own', () => {
		expect(sectionFromHash('#workflows')).toBeNull();
	});

	it('is null for an empty or missing hash', () => {
		expect(sectionFromHash('')).toBeNull();
		expect(sectionFromHash(null)).toBeNull();
		expect(sectionFromHash(undefined)).toBeNull();
	});
});

describe('scrollToSection', () => {
	it('scrolls to the card the hash names', () => {
		const scrollIntoView = vi.fn();
		const find = vi.fn(() => ({ scrollIntoView }));
		expect(scrollToSection('#mcp', true, find)).toBe('mcp');
		expect(find).toHaveBeenCalledWith('mcp');
		expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'smooth', block: 'start' });
	});

	it('waits until the settings have loaded', () => {
		const find = vi.fn(() => ({ scrollIntoView: vi.fn() }));
		expect(scrollToSection('#mcp', false, find)).toBeNull();
		expect(find).not.toHaveBeenCalled();
	});

	it('does nothing for an unknown hash', () => {
		const find = vi.fn(() => ({ scrollIntoView: vi.fn() }));
		expect(scrollToSection('#nowhere', true, find)).toBeNull();
		expect(find).not.toHaveBeenCalled();
	});

	it('survives a card that is not in the document yet', () => {
		expect(scrollToSection('#daemon', true, () => null)).toBeNull();
	});

	it('survives a webview with no smooth scrolling', () => {
		expect(scrollToSection('#daemon', true, () => ({}))).toBe('daemon');
	});
});
