// The nine rail destinations. Board is not one of them: it belongs to a project, so
// `/board` keeps the Projects item lit.

import { comboKeys, type Platform } from '$lib/ds';
import type { IconName } from '$lib/ds';

export type ViewId =
	| 'dashboard'
	| 'projects'
	| 'memories'
	| 'agents'
	| 'practices'
	| 'workflows'
	| 'review'
	| 'mcp'
	| 'settings'
	/** Not a rail item: `/plugins` is reached from Settings and the palette, but it owns
	 * a side panel of its own, so it needs a view id. */
	| 'plugins';

export interface ViewDef {
	id: ViewId;
	label: string;
	icon: IconName;
	href: string;
	/** Platform-neutral combo; `shortcutText` renders it. */
	combo: string;
}

/** Rail order. Settings sits at the bottom of the rail but is one of these. */
export const VIEWS: ViewDef[] = [
	{ id: 'dashboard', label: 'Dashboard', icon: 'gauge', href: '/', combo: 'Mod+1' },
	{ id: 'projects', label: 'Projects', icon: 'folder', href: '/projects', combo: 'Mod+2' },
	{ id: 'memories', label: 'Memories', icon: 'database', href: '/memories', combo: 'Mod+3' },
	{ id: 'agents', label: 'Agents', icon: 'bot', href: '/agents', combo: 'Mod+4' },
	{ id: 'practices', label: 'Practices', icon: 'book-open', href: '/practices', combo: 'Mod+5' },
	{ id: 'workflows', label: 'Workflows', icon: 'git-branch', href: '/workflows', combo: 'Mod+6' },
	{ id: 'review', label: 'Review', icon: 'list-checks', href: '/review', combo: 'Mod+7' },
	{ id: 'mcp', label: 'MCP', icon: 'plug', href: '/mcp', combo: 'Mod+8' },
	{ id: 'settings', label: 'Settings', icon: 'settings', href: '/settings', combo: 'Mod+,' }
];

/** The seven the rail stacks at the top; Settings is drawn separately at the bottom. */
export const MAIN_VIEWS = VIEWS.filter((v) => v.id !== 'settings');
export const SETTINGS_VIEW = VIEWS[VIEWS.length - 1];

/** The side panel title per view. Dashboard has no side panel. */
const PANEL_TITLES: Record<ViewId, string> = {
	dashboard: '',
	projects: 'Projects',
	memories: 'Memory filters',
	agents: 'Agents',
	practices: 'Practices',
	workflows: 'Workflows',
	review: 'Review',
	mcp: 'MCP',
	settings: 'Settings',
	plugins: 'Plugins'
};

export function panelTitle(view: ViewId): string {
	return PANEL_TITLES[view];
}

export function viewLabel(view: ViewId): string {
	if (view === 'plugins') return PANEL_TITLES.plugins;
	return VIEWS.find((v) => v.id === view)?.label ?? 'Dashboard';
}

/** The rail item a route lights up. `/board` is project work, so it lights Projects.
 * `/plugins` lights nothing: it is not a rail destination, only a side panel owner. */
export function viewForPath(path: string): ViewId {
	if (path === '/') return 'dashboard';
	if (path === '/board' || path.startsWith('/board/')) return 'projects';
	if (path === '/plugins' || path.startsWith('/plugins/')) return 'plugins';
	const hit = VIEWS.find(
		(v) => v.href !== '/' && (path === v.href || path.startsWith(`${v.href}/`))
	);
	return hit?.id ?? 'dashboard';
}

/** `⌘1` on mac, `Ctrl+1` elsewhere. */
export function shortcutText(combo: string, platform: Platform): string {
	const keys = comboKeys(combo, platform);
	return platform === 'mac' ? keys.join('') : keys.join('+');
}
