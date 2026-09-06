// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';

import {
	applyFonts,
	fontMonoStack,
	fontUiStack,
	FONT_MONO_OPTIONS,
	FONT_SIZE_OPTIONS,
	FONT_UI_OPTIONS,
	isMonospace,
	MONO_SIZE_OPTIONS
} from './fonts';

describe('fontUiStack / fontMonoStack', () => {
	it('resolves every preset to a distinct stack', () => {
		expect(fontUiStack('system')).toContain('system-ui');
		expect(fontUiStack('inter')).toContain('Inter');
		expect(fontUiStack('jetbrains-mono')).toContain('JetBrains Mono');
		expect(fontMonoStack('jetbrains-mono')).toContain('JetBrains Mono');
		expect(fontMonoStack('system-mono')).toContain('ui-monospace');
	});

	it('puts an installed family, quoted, ahead of the system stack', () => {
		expect(fontUiStack('SF Pro')).toBe('"SF Pro",-apple-system,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif');
		expect(fontMonoStack('Zed Mono')).toBe('"Zed Mono",ui-monospace,"SF Mono","Cascadia Code",Menlo,Consolas,monospace');
		expect(fontUiStack('Odd "Name"')).toContain('"Odd \\"Name\\""');
	});

	it('option lists name the presets and the size stops', () => {
		expect(FONT_UI_OPTIONS.map((o) => o.value)).toEqual(['system', 'inter']);
		expect(FONT_MONO_OPTIONS.map((o) => o.value)).toEqual(['jetbrains-mono', 'system-mono']);
		expect(FONT_SIZE_OPTIONS.map((o) => o.value)).toEqual(['11', '12', '13', '14', '15', '16']);
		expect(MONO_SIZE_OPTIONS.map((o) => o.value)).toEqual(['10', '11', '12', '13', '14', '15', '16']);
	});
});

describe('applyFonts', () => {
	it('sets the font and size custom properties on documentElement', () => {
		applyFonts('inter', 'system-mono', 13, 14, true);
		const root = document.documentElement;
		expect(root.style.getPropertyValue('--font-ui')).toContain('Inter');
		expect(root.style.getPropertyValue('--font-mono')).toContain('ui-monospace');
		expect(root.style.getPropertyValue('--text-sm')).toBe('13px');
		expect(root.style.getPropertyValue('--mono-sm')).toBe('14px');
		expect(root.dataset.smoothing).toBeUndefined();
	});

	it('marks the document when smoothing is off, and clears the mark when on again', () => {
		applyFonts('system', 'jetbrains-mono', 12, 12, false);
		expect(document.documentElement.dataset.smoothing).toBe('off');
		applyFonts('system', 'jetbrains-mono', 12, 12, true);
		expect(document.documentElement.dataset.smoothing).toBeUndefined();
	});
});

describe('isMonospace', () => {
	it('trusts the flag and falls back to the names monospace fonts carry', () => {
		expect(isMonospace({ family: 'Menlo', monospace: true })).toBe(true);
		expect(isMonospace({ family: 'Fira Code', monospace: false })).toBe(true);
		expect(isMonospace({ family: 'IBM Plex Mono', monospace: false })).toBe(true);
		expect(isMonospace({ family: 'SF Pro', monospace: false })).toBe(false);
		expect(isMonospace({ family: 'Monoton', monospace: false })).toBe(false);
	});
});
