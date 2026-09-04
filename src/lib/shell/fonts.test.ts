// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';

import { applyFonts, fontMonoStack, fontUiStack, FONT_MONO_OPTIONS, FONT_SIZE_OPTIONS, FONT_UI_OPTIONS } from './fonts';

describe('fontUiStack / fontMonoStack', () => {
	it('resolves every UI font option to a distinct stack', () => {
		expect(fontUiStack('system')).toContain('system-ui');
		expect(fontUiStack('inter')).toContain('Inter');
		expect(fontUiStack('jetbrains-mono')).toContain('JetBrains Mono');
	});

	it('resolves every mono font option to a distinct stack', () => {
		expect(fontMonoStack('jetbrains-mono')).toContain('JetBrains Mono');
		expect(fontMonoStack('system-mono')).toContain('ui-monospace');
	});

	it('option lists cover every value the settings validate', () => {
		expect(FONT_UI_OPTIONS.map((o) => o.value)).toEqual(['system', 'inter', 'jetbrains-mono']);
		expect(FONT_MONO_OPTIONS.map((o) => o.value)).toEqual(['jetbrains-mono', 'system-mono']);
		expect(FONT_SIZE_OPTIONS.map((o) => o.value)).toEqual(['11', '12', '13']);
	});
});

describe('applyFonts', () => {
	it('sets the font and size custom properties on documentElement', () => {
		applyFonts('inter', 'system-mono', 13);
		const style = document.documentElement.style;
		expect(style.getPropertyValue('--font-ui')).toContain('Inter');
		expect(style.getPropertyValue('--font-mono')).toContain('ui-monospace');
		expect(style.getPropertyValue('--text-sm')).toBe('13px');
	});
});
