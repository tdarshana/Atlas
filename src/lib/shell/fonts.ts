// Font choices for the Appearance settings card: a UI family, a mono family and a base
// UI text size. Applied as inline custom properties on `documentElement`, so they win
// over the `--font-ui`/`--font-mono`/`--text-sm` defaults `fonts.css`/`typography.css`
// set at `:root` without touching those files. The stacks named here duplicate the app
// boot script in `src/app.html`, which cannot import this module: it runs before the
// bundle does, to paint the right fonts before first paint.

export type FontUi = 'system' | 'inter' | 'jetbrains-mono';
export type FontMono = 'jetbrains-mono' | 'system-mono';
export type FontSize = 11 | 12 | 13;

export const FONT_UI_OPTIONS: { value: FontUi; label: string }[] = [
	{ value: 'system', label: 'System' },
	{ value: 'inter', label: 'Inter' },
	{ value: 'jetbrains-mono', label: 'JetBrains Mono' }
];

export const FONT_MONO_OPTIONS: { value: FontMono; label: string }[] = [
	{ value: 'jetbrains-mono', label: 'JetBrains Mono' },
	{ value: 'system-mono', label: 'System mono' }
];

export const FONT_SIZE_OPTIONS: { value: string; label: string }[] = [
	{ value: '11', label: '11' },
	{ value: '12', label: '12 (default)' },
	{ value: '13', label: '13' }
];

const FONT_UI_STACKS: Record<FontUi, string> = {
	system: '-apple-system,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif',
	inter: 'Inter,-apple-system,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif',
	'jetbrains-mono': '"JetBrains Mono","SF Mono","Cascadia Code",Menlo,Consolas,monospace'
};

const FONT_MONO_STACKS: Record<FontMono, string> = {
	'jetbrains-mono': '"JetBrains Mono","SF Mono","Cascadia Code",Menlo,Consolas,monospace',
	'system-mono': 'ui-monospace,"SF Mono","Cascadia Code",Menlo,Consolas,monospace'
};

export function fontUiStack(choice: FontUi): string {
	return FONT_UI_STACKS[choice];
}

export function fontMonoStack(choice: FontMono): string {
	return FONT_MONO_STACKS[choice];
}

/** Applies the three font settings to `documentElement`. No-op outside the browser. */
export function applyFonts(fontUi: FontUi, fontMono: FontMono, fontSize: FontSize): void {
	if (typeof document === 'undefined') return;
	const root = document.documentElement.style;
	root.setProperty('--font-ui', fontUiStack(fontUi));
	root.setProperty('--font-mono', fontMonoStack(fontMono));
	root.setProperty('--text-sm', `${fontSize}px`);
}
