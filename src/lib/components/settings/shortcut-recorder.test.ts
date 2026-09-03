import { describe, expect, it } from 'vitest';
import { acceleratorToKeyHintCombo, comboFromEvent, comboToAccelerator } from './shortcut-recorder';

describe('comboFromEvent', () => {
	it('captures the platform modifier as CmdOrCtrl on mac (metaKey)', () => {
		const combo = comboFromEvent({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, key: 'k' });
		expect(combo).toEqual({ modifiers: ['CmdOrCtrl'], key: 'K' });
	});

	it('captures the platform modifier as CmdOrCtrl on windows/linux (ctrlKey)', () => {
		const combo = comboFromEvent({ metaKey: false, ctrlKey: true, altKey: false, shiftKey: false, key: 'k' });
		expect(combo.modifiers).toEqual(['CmdOrCtrl']);
	});

	it('orders modifiers CmdOrCtrl, Alt, Shift regardless of press order', () => {
		const combo = comboFromEvent({ metaKey: true, ctrlKey: false, altKey: true, shiftKey: true, key: 'p' });
		expect(combo.modifiers).toEqual(['CmdOrCtrl', 'Alt', 'Shift']);
	});

	it('has no key yet while only a modifier is held', () => {
		const combo = comboFromEvent({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, key: 'Meta' });
		expect(combo.key).toBeNull();
	});

	it('uppercases a single-character key and spells out Space', () => {
		expect(comboFromEvent({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, key: 'j' }).key).toBe('J');
		expect(comboFromEvent({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, key: ' ' }).key).toBe('Space');
	});

	it('keeps a named key as-is', () => {
		expect(comboFromEvent({ metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, key: 'Escape' }).key).toBe('Escape');
	});
});

describe('comboToAccelerator', () => {
	it('joins modifiers and the key with +', () => {
		expect(comboToAccelerator({ modifiers: ['CmdOrCtrl', 'Shift'], key: 'K' })).toBe('CmdOrCtrl+Shift+K');
	});

	it('is null with no key yet', () => {
		expect(comboToAccelerator({ modifiers: ['CmdOrCtrl'], key: null })).toBeNull();
	});

	it('is null with no modifier: a bare key is never a valid global shortcut', () => {
		expect(comboToAccelerator({ modifiers: [], key: 'K' })).toBeNull();
	});
});

describe('acceleratorToKeyHintCombo', () => {
	it('renders CmdOrCtrl as the Mod token KeyHint understands', () => {
		expect(acceleratorToKeyHintCombo('CmdOrCtrl+Shift+K')).toBe('Mod+Shift+K');
	});

	it('leaves every other part alone', () => {
		expect(acceleratorToKeyHintCombo('Alt+Space')).toBe('Alt+Space');
	});
});
