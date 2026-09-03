/* The window chrome: title bar, activity rail, side panel, status bar and the pieces
   pages compose inside the content panel. */

export { default as TitleBar } from './TitleBar.svelte';
export { default as ActivityRail } from './ActivityRail.svelte';
export { default as SidePanel } from './SidePanel.svelte';
export { default as StatusBar } from './StatusBar.svelte';
export { default as Panel } from './Panel.svelte';
export { default as SectionHeading } from './SectionHeading.svelte';
export { default as TabStrip, type Tab } from './TabStrip.svelte';
export { default as TreeGroup } from './TreeGroup.svelte';
export { default as TreeRow } from './TreeRow.svelte';

export {
	shell,
	setTheme,
	setView,
	setStatusItems,
	toggleRail,
	toggleSidePanel,
	initShell,
	applyDaemonTheme,
	RAIL_KEY,
	SIDEPANEL_KEY,
	THEME_KEY,
	type Theme,
	type StatusItem,
	type StatusItems
} from './shell.svelte';
export { installShortcuts, PALETTE_EVENT } from './shortcuts';
export { resolvePlatform, inTauri } from './platform';
export { VIEWS, MAIN_VIEWS, viewForPath, viewLabel, panelTitle, type ViewId } from './views';
