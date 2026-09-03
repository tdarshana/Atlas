// The Settings screen's cards, and the hash the side panel's Sections rows link to.
// Keeping the ids in one list means the panel and the page cannot drift apart, and the
// scroll helper never hands `getElementById` a fragment the page does not own.

export interface SettingsSection {
	id: string;
	label: string;
}

export const SETTINGS_SECTIONS: SettingsSection[] = [
	{ id: 'daemon', label: 'Daemon' },
	{ id: 'extraction', label: 'Extraction' },
	{ id: 'board-stages', label: 'Board stages' },
	{ id: 'mcp', label: 'MCP server' }
];

/**
 * The section a URL hash names, or null when it names none. A leading `#` is optional so
 * both `page.url.hash` and a bare id work.
 */
export function sectionFromHash(hash: string | null | undefined): string | null {
	if (!hash) return null;
	const id = hash.startsWith('#') ? hash.slice(1) : hash;
	return SETTINGS_SECTIONS.some((s) => s.id === id) ? id : null;
}

/**
 * Scrolls to the card the hash names, once the page has something to scroll to. Returns
 * the id it scrolled to, or null when there was nothing to do, so a caller can tell the
 * two apart. jsdom and older webviews have no `scrollIntoView`; the anchor still resolves.
 */
export function scrollToSection(
	hash: string | null | undefined,
	loaded: boolean,
	find: (id: string) => { scrollIntoView?: (options?: ScrollIntoViewOptions) => void } | null
): string | null {
	if (!loaded) return null;
	const id = sectionFromHash(hash);
	if (!id) return null;
	const el = find(id);
	if (!el) return null;
	el.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
	return id;
}
