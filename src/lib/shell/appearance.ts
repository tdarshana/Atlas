// Applies and mirrors the Appearance settings card's draft: the base theme, an
// optional theme pack, the three font settings and the UI scale. Mirrors every value
// into localStorage under the same keys `src/app.html`'s boot script reads, and into
// the Tauri UI-state store through `persistSet`, so the next launch paints correctly
// before the daemon answers. Does not touch the daemon itself; the settings page sends
// the same values there alongside calling this.

import type { ThemePack } from '$lib/types';
import type { FontMono, FontSize, FontUi } from './fonts';
import { applyFonts } from './fonts';
import { persistSet } from '$lib/platform/persist';
import { shell, THEME_KEY, type Theme } from './shell.svelte';
import { applyThemePack, clearThemePack } from './theme-pack';

export const THEME_PACK_KEY = 'atlas.theme_pack';
export const FONT_UI_KEY = 'atlas.font_ui';
export const FONT_MONO_KEY = 'atlas.font_mono';
export const FONT_SIZE_KEY = 'atlas.font_size';
export const SCALE_KEY = 'atlas.scale';

/** The whole app's zoom level, an integer percent. Not a font concern, so it lives here
 * rather than in `./fonts`. */
export type UiScale = 80 | 90 | 100 | 110 | 125 | 150;

/** The stops, in order; the scale shortcuts step along this list. */
export const SCALE_STOPS: readonly UiScale[] = [80, 90, 100, 110, 125, 150];

export const SCALE_OPTIONS: { value: string; label: string }[] = SCALE_STOPS.map((v) => ({
	value: String(v),
	label: `${v}%`
}));

/** The scale the document is at now, read back from the root's zoom; 100 when unset. */
export function currentScale(): UiScale {
	if (typeof document === 'undefined') return 100;
	const zoom = parseFloat(document.documentElement.style.zoom);
	const pct = Number.isFinite(zoom) && zoom > 0 ? Math.round(zoom * 100) : 100;
	return (SCALE_STOPS as readonly number[]).includes(pct) ? (pct as UiScale) : 100;
}

/** Zooms the document to one stop and remembers it locally. The daemon's `ui.scale`
 * is the caller's to save, so a shortcut and the Appearance card share this. */
export function applyScale(scale: UiScale): void {
	if (typeof document !== 'undefined') {
		// `--ui-zoom` rides along for the body rule in the root layout: WebKit's zoom gets
		// a fixed body's bottom edge wrong, so the body divides its height by this instead.
		const root = document.documentElement.style;
		if (scale === 100) {
			// An empty string clears the property (and, unlike removeProperty, also in jsdom).
			root.zoom = '';
			root.removeProperty('--ui-zoom');
		} else {
			root.zoom = String(scale / 100);
			root.setProperty('--ui-zoom', String(scale / 100));
		}
	}
	writeStored(SCALE_KEY, scale === 100 ? null : String(scale));
}

/** The stop after (or before) the current one, or null at either end of the list. */
export function nextScale(direction: 1 | -1): UiScale | null {
	return SCALE_STOPS[SCALE_STOPS.indexOf(currentScale()) + direction] ?? null;
}

function writeStored(key: string, value: string | null): void {
	try {
		if (typeof localStorage === 'undefined') return;
		if (value === null) localStorage.removeItem(key);
		else localStorage.setItem(key, value);
	} catch {
		/* storage is unavailable; the preference is simply not remembered */
	}
	void persistSet(key, value);
}

/**
 * Applies a full appearance draft to the live document (base theme, pack tokens, then
 * fonts) and mirrors it into localStorage. Called once, on Save, so nothing changes
 * document-wide before then; the Appearance card's live preview is a separate, local
 * effect.
 */
export function applyAppearance(
	base: Theme,
	pack: ThemePack | null,
	fontUi: FontUi,
	fontMono: FontMono,
	fontSize: FontSize,
	scale: UiScale
): void {
	shell.theme = base;
	if (typeof document !== 'undefined') document.documentElement.dataset.theme = base;
	applyScale(scale);
	if (pack) applyThemePack(pack);
	else clearThemePack();
	applyFonts(fontUi, fontMono, fontSize);

	writeStored(THEME_KEY, base);
	writeStored(THEME_PACK_KEY, pack ? JSON.stringify(pack) : null);
	writeStored(FONT_UI_KEY, fontUi);
	writeStored(FONT_MONO_KEY, fontMono);
	writeStored(FONT_SIZE_KEY, String(fontSize));
}
