// Shell chrome state: which view is lit, whether the rail is expanded, whether the side
// panel is showing, the theme and the platform. Rail, side panel and theme survive a
// restart in localStorage; the theme is also mirrored into the daemon's `ui.theme` so a
// second client agrees. Every storage touch is guarded: a webview with site data blocked
// throws on access, and losing a preference must never take the window down.

import { api } from '$lib/daemon.svelte';
import type { Platform } from '$lib/ds';
import { UI_THEME_KEY } from '$lib/types';
import { resolvePlatform } from './platform';
import { panelTitle, type ViewId } from './views';

export type Theme = 'dark' | 'light';
export type StatusTone = 'success' | 'warning' | 'danger';

export interface StatusItem {
	text: string;
	tone?: StatusTone;
}

export interface StatusItems {
	left: string[];
	right: StatusItem[];
}

export const RAIL_KEY = 'atlas.rail';
export const SIDEPANEL_KEY = 'atlas.sidepanel';
export const THEME_KEY = 'atlas.theme';

function readStored(key: string): string | null {
	try {
		if (typeof localStorage === 'undefined') return null;
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function writeStored(key: string, value: string): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(key, value);
	} catch {
		/* storage is unavailable; the preference is simply not remembered */
	}
}

export const shell = $state({
	railExpanded: readStored(RAIL_KEY) === 'expanded',
	sidePanel: readStored(SIDEPANEL_KEY) !== 'hidden',
	theme: (readStored(THEME_KEY) === 'light' ? 'light' : 'dark') as Theme,
	/** The design is authored for mac chrome; Tauri corrects this on mount. */
	platform: 'mac' as Platform,
	view: 'dashboard' as ViewId,
	sidePanelTitle: '',
	/** Written by the active page; the status bar renders whatever it finds. */
	statusItems: { left: [], right: [] } as StatusItems
});

export function toggleRail(): void {
	shell.railExpanded = !shell.railExpanded;
	writeStored(RAIL_KEY, shell.railExpanded ? 'expanded' : 'collapsed');
}

export function toggleSidePanel(): void {
	shell.sidePanel = !shell.sidePanel;
	writeStored(SIDEPANEL_KEY, shell.sidePanel ? 'shown' : 'hidden');
}

/** Applies the theme everywhere: the root element, localStorage and the daemon. */
export function setTheme(theme: Theme): void {
	shell.theme = theme;
	if (typeof document !== 'undefined') document.documentElement.dataset.theme = theme;
	writeStored(THEME_KEY, theme);
	void saveTheme(theme);
}

async function saveTheme(theme: Theme): Promise<void> {
	try {
		await api().setSettings({ [UI_THEME_KEY]: theme });
	} catch {
		/* the daemon may be down; localStorage already holds the choice */
	}
}

/**
 * Reconciles the boot theme with the daemon's. The daemon is the shared record, so its
 * value wins when the two disagree; writing it back would be a loop.
 */
export function applyDaemonTheme(value: unknown): void {
	const theme = value === 'light' || value === 'dark' ? value : null;
	if (!theme || theme === shell.theme) return;
	shell.theme = theme;
	if (typeof document !== 'undefined') document.documentElement.dataset.theme = theme;
	writeStored(THEME_KEY, theme);
}

/** Called once on mount: the inline script in `app.html` already painted the theme. */
export async function initShell(): Promise<void> {
	if (typeof document !== 'undefined') document.documentElement.dataset.theme = shell.theme;
	shell.platform = await resolvePlatform();
}

/**
 * Called by the layout whenever the route changes. The status items belong to the page
 * that set them, so they are dropped here: without this a page that sets none would show
 * the last page's counts.
 */
export function setView(view: ViewId): void {
	shell.view = view;
	shell.sidePanelTitle = panelTitle(view);
	shell.statusItems = { left: [], right: [] };
}

/** Pages call this to fill the right-hand end of the status bar. */
export function setStatusItems(items: Partial<StatusItems>): void {
	shell.statusItems = { left: items.left ?? [], right: items.right ?? [] };
}
