// Applies and mirrors the Appearance settings card's draft: the base theme, an
// optional theme pack and the three font settings. Mirrors every value into
// localStorage under the same keys `src/app.html`'s boot script reads, and into the
// Tauri UI-state store through `persistSet`, so the next launch paints correctly
// before the daemon answers. Does not touch the daemon itself; the settings page sends
// the same values there alongside calling this.

import type { ThemePack } from '$lib/types';
import type { FontMono, FontSize, FontUi } from './fonts';
import { applyFonts } from './fonts';
import { persistSet } from './persist';
import { shell, THEME_KEY, type Theme } from './shell.svelte';
import { applyThemePack, clearThemePack } from './theme-pack';

export const THEME_PACK_KEY = 'atlas.theme_pack';
export const FONT_UI_KEY = 'atlas.font_ui';
export const FONT_MONO_KEY = 'atlas.font_mono';
export const FONT_SIZE_KEY = 'atlas.font_size';

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
export function applyAppearance(base: Theme, pack: ThemePack | null, fontUi: FontUi, fontMono: FontMono, fontSize: FontSize): void {
	shell.theme = base;
	if (typeof document !== 'undefined') document.documentElement.dataset.theme = base;
	if (pack) applyThemePack(pack);
	else clearThemePack();
	applyFonts(fontUi, fontMono, fontSize);

	writeStored(THEME_KEY, base);
	writeStored(THEME_PACK_KEY, pack ? JSON.stringify(pack) : null);
	writeStored(FONT_UI_KEY, fontUi);
	writeStored(FONT_MONO_KEY, fontMono);
	writeStored(FONT_SIZE_KEY, String(fontSize));
}
