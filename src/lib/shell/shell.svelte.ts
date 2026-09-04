// Shell chrome state: which view is lit, whether the rail is expanded, whether the side
// panel is showing, the theme and the platform. Rail, side panel and theme survive a
// restart in localStorage; the theme is also mirrored into the daemon's `ui.theme` so a
// second client agrees. Every storage touch is guarded: a webview with site data blocked
// throws on access, and losing a preference must never take the window down.

import type { Component } from 'svelte';
import { api } from '$lib/daemon.svelte';
import type { Platform } from '$lib/ds';
import { UI_THEME_KEY } from '$lib/types';
import { migrateLocalStorage, persistSet } from './persist';
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

/**
 * A page can lend the side panel its own body: the Board tab replaces the Projects list
 * with its filters. The panel keeps its own title while the override stands.
 */
export interface SidePanelOverride {
	title: string;
	component: Component;
}

export const RAIL_KEY = 'atlas.rail';
export const SIDEPANEL_KEY = 'atlas.sidepanel';
export const SIDEPANEL_WIDTH_KEY = 'atlas.sidepanel.width';
/** The side panel is 220px in the design and drags between these two. */
export const SIDEPANEL_DEFAULT = 220;
export const SIDEPANEL_MIN = 180;
export const SIDEPANEL_MAX = 400;
const clampSide = (w: number) => Math.min(SIDEPANEL_MAX, Math.max(SIDEPANEL_MIN, Math.round(w)));
function readSideWidth(): number {
	const n = Number(readStored(SIDEPANEL_WIDTH_KEY));
	return Number.isFinite(n) && n > 0 ? clampSide(n) : SIDEPANEL_DEFAULT;
}
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
	void persistSet(key, value);
}

export const shell = $state({
	railExpanded: readStored(RAIL_KEY) === 'expanded',
	sidePanel: readStored(SIDEPANEL_KEY) !== 'hidden',
	sidePanelWidth: readSideWidth(),
	theme: (readStored(THEME_KEY) === 'light' ? 'light' : 'dark') as Theme,
	/** The design is authored for mac chrome; Tauri corrects this on mount. */
	platform: 'mac' as Platform,
	view: 'dashboard' as ViewId,
	sidePanelTitle: '',
	/** Set by the page that wants its own side panel body; null is the view's own. */
	sidePanelOverride: null as SidePanelOverride | null,
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

export function setSidePanelWidth(width: number): void {
	shell.sidePanelWidth = clampSide(width);
	writeStored(SIDEPANEL_WIDTH_KEY, String(shell.sidePanelWidth));
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
	void migrateLocalStorage();
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

/**
 * Lends the side panel a body. Unlike the status items this is not dropped by `setView`:
 * the page that set it clears it when it is torn down, which happens after the incoming
 * page has already claimed the panel on a move between two boards.
 */
export function setSidePanelOverride(override: SidePanelOverride): void {
	shell.sidePanelOverride = override;
}

export function clearSidePanelOverride(): void {
	shell.sidePanelOverride = null;
}
