// Font choices for the Appearance card's Typography section: an interface family, a
// monospace family, their two sizes and the smoothing switch. Applied as inline custom
// properties (and one data attribute) on `documentElement`, so they win over the
// `--font-ui`/`--font-mono`/`--text-sm`/`--mono-sm` defaults `fonts.css` and
// `typography.css` set at `:root` without touching those files. A family is either one
// of the bundled presets below or the name of a font installed on the machine, which the
// host lists through `list_fonts`. The stacks named here duplicate the app boot script in
// `src/app.html`, which cannot import this module: it runs before the bundle does, to
// paint the right fonts before first paint.

import { inTauri } from './platform';

/** A preset name or an installed family name. */
export type FontUi = string;
export type FontMono = string;
export type FontSize = number;

export const DEFAULT_FONT_UI = 'system';
export const DEFAULT_FONT_MONO = 'jetbrains-mono';
export const DEFAULT_FONT_SIZE = 12;
export const DEFAULT_MONO_SIZE = 12;

export const FONT_UI_OPTIONS: { value: string; label: string; hint?: string }[] = [
	{ value: 'system', label: 'System', hint: 'default' },
	{ value: 'inter', label: 'Inter' },
	{ value: 'jetbrains-mono', label: 'JetBrains Mono' }
];

export const FONT_MONO_OPTIONS: { value: string; label: string; hint?: string }[] = [
	{ value: 'jetbrains-mono', label: 'JetBrains Mono', hint: 'default' },
	{ value: 'system-mono', label: 'System mono' }
];

/** Interface text sizes, in px; the app is laid out for 12. */
export const FONT_SIZES: readonly number[] = [11, 12, 13, 14, 15, 16];
export const FONT_SIZE_OPTIONS = FONT_SIZES.map((v) => ({ value: String(v), label: `${v} px` }));

/** Code text sizes, in px. */
export const MONO_SIZES: readonly number[] = [10, 11, 12, 13, 14, 15, 16];
export const MONO_SIZE_OPTIONS = MONO_SIZES.map((v) => ({ value: String(v), label: `${v} px` }));

const SYSTEM_UI = '-apple-system,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';
const SYSTEM_MONO = 'ui-monospace,"SF Mono","Cascadia Code",Menlo,Consolas,monospace';

const FONT_UI_STACKS: Record<string, string> = {
	system: SYSTEM_UI,
	inter: `Inter,${SYSTEM_UI}`,
	'jetbrains-mono': '"JetBrains Mono","SF Mono","Cascadia Code",Menlo,Consolas,monospace'
};

const FONT_MONO_STACKS: Record<string, string> = {
	'jetbrains-mono': '"JetBrains Mono","SF Mono","Cascadia Code",Menlo,Consolas,monospace',
	'system-mono': SYSTEM_MONO
};

/** A family name quoted for a `font-family` list. */
function quoted(family: string): string {
	return `"${family.replace(/["\\]/g, '\\$&')}"`;
}

/** The stack for a preset, or an installed family ahead of the system UI stack. */
export function fontUiStack(choice: FontUi): string {
	return FONT_UI_STACKS[choice] ?? `${quoted(choice)},${SYSTEM_UI}`;
}

/** The stack for a preset, or an installed family ahead of the system mono stack. */
export function fontMonoStack(choice: FontMono): string {
	return FONT_MONO_STACKS[choice] ?? `${quoted(choice)},${SYSTEM_MONO}`;
}

/**
 * Applies the typography settings to `documentElement`. Smoothing on is the app's
 * thinner grayscale anti-aliasing (`base.css`); off leaves WebKit's default, which
 * `html[data-smoothing="off"]` restores. No-op outside the browser.
 */
export function applyFonts(
	fontUi: FontUi,
	fontMono: FontMono,
	fontSize: FontSize,
	monoSize: FontSize = DEFAULT_MONO_SIZE,
	smoothing = true
): void {
	if (typeof document === 'undefined') return;
	const root = document.documentElement;
	root.style.setProperty('--font-ui', fontUiStack(fontUi));
	root.style.setProperty('--font-mono', fontMonoStack(fontMono));
	root.style.setProperty('--text-sm', `${fontSize}px`);
	root.style.setProperty('--mono-sm', `${monoSize}px`);
	applySmoothing(smoothing);
}

/** Switches the anti-aliasing alone, for the toggle that applies as it is flipped. */
export function applySmoothing(on: boolean): void {
	if (typeof document === 'undefined') return;
	if (on) delete document.documentElement.dataset.smoothing;
	else document.documentElement.dataset.smoothing = 'off';
}

export interface InstalledFont {
	family: string;
	/** True when every face of the family is monospaced. */
	monospace: boolean;
}

/** The families installed on this machine, from the host; empty in a plain browser. */
export async function loadInstalledFonts(): Promise<InstalledFont[]> {
	if (!inTauri()) return [];
	try {
		const { invoke } = await import('@tauri-apps/api/core');
		return await invoke<InstalledFont[]>('list_fonts');
	} catch {
		return [];
	}
}
