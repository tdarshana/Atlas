// The palette's command list. A command either navigates (the target page reads the
// query parameter and opens the right thing) or drives the shell directly. Only the
// combos the shell actually binds are printed, so a hint never promises a key that
// does nothing.

import { comboKeys, type IconName } from '$lib/ds';
import { type Contributions, sectionHref } from '$lib/plugins/contributions';
import { dispatchCommand } from '$lib/plugins/host.svelte';
import { desktop, inTauri } from '$lib/shell/platform';
import { setTheme, shell, toggleRail, toggleSidePanel } from '$lib/shell/shell.svelte';
import type { Uuid } from '$lib/types';
import { push } from '$lib/ui/toasts.svelte';

/** Opens the log directory in the OS file manager. No-op outside Tauri: there is no
 * folder to reveal in a browser tab, so `desktop` is skipped entirely rather than
 * warning for a command that has nothing useful to fall back to. Exported so the
 * Settings screen's About card can offer the same action as its own button. */
export async function openLogFolder(): Promise<void> {
	if (!inTauri()) return;
	await desktop('open_log_folder', undefined, () => undefined);
}

export interface CommandContext {
	/** The project the window is on, or null when the view is global. */
	projectId: Uuid | null;
	goto: (href: string) => unknown;
}

export interface PaletteCommand {
	id: string;
	label: string;
	/** The dim line after the label; empty when the label says it all. */
	hint: string;
	icon: IconName;
	/** A logical combo the shell binds, rendered with `KeyHint`. */
	combo?: string;
	/** True for a command that only means anything inside the desktop app (window
	 * placement, quitting the process): hidden outside Tauri rather than listed as a
	 * command that logs a fallback warning and does nothing. */
	desktopOnly?: boolean;
	run: (ctx: CommandContext) => unknown;
}

export const COMMANDS: PaletteCommand[] = [
	{
		id: 'new-task',
		label: 'New task…',
		hint: '',
		icon: 'plus',
		// Off a project the new task belongs to the every-project board, not to nothing.
		run: (ctx) => ctx.goto(`/projects/${ctx.projectId ?? 'global'}/board?new=1`)
	},
	{
		id: 'remember',
		label: 'Remember…',
		hint: 'add a memory by hand',
		icon: 'database',
		run: (ctx) => ctx.goto('/memories?remember=1')
	},
	{
		id: 'connect-folder',
		label: 'Connect a folder…',
		hint: '',
		icon: 'plug',
		run: (ctx) => ctx.goto('/projects?connect=1')
	},
	{
		id: 'sync-agents',
		label: 'Sync agents',
		hint: 'global scope',
		icon: 'refresh-cw',
		run: (ctx) => ctx.goto('/agents?sync=1')
	},
	{
		id: 'run-workflow',
		label: 'Run workflow…',
		hint: '',
		icon: 'play',
		run: (ctx) => ctx.goto('/workflows')
	},
	{
		id: 'open-mcp',
		label: 'Open MCP',
		hint: '',
		icon: 'plug',
		combo: 'Mod+8',
		run: (ctx) => ctx.goto('/mcp')
	},
	{
		id: 'open-plugins',
		label: 'Plugins',
		hint: 'install and enable plugins',
		icon: 'plug',
		run: (ctx) => ctx.goto('/plugins')
	},
	{
		id: 'open-settings',
		label: 'Open settings',
		hint: '',
		icon: 'settings',
		combo: 'Mod+,',
		run: (ctx) => ctx.goto('/settings')
	},
	{
		id: 'toggle-theme',
		label: 'Toggle theme',
		hint: 'dark and light',
		icon: 'wand-sparkles',
		run: () => setTheme(shell.theme === 'dark' ? 'light' : 'dark')
	},
	{
		id: 'toggle-rail',
		label: 'Toggle rail',
		hint: '',
		icon: 'panel-left',
		combo: 'Mod+B',
		run: () => toggleRail()
	},
	{
		id: 'toggle-side-panel',
		label: 'Toggle side panel',
		hint: '',
		icon: 'panel-right',
		combo: 'Mod+J',
		run: () => toggleSidePanel()
	},
	{
		id: 'window-center',
		label: 'Move window to center',
		hint: '',
		icon: 'maximize',
		desktopOnly: true,
		run: () => desktop('window_center', undefined, () => undefined)
	},
	{
		id: 'window-top-left',
		label: 'Move window to top left',
		hint: '',
		icon: 'maximize',
		desktopOnly: true,
		run: () => desktop('window_move', { position: 'top-left' }, () => undefined)
	},
	{
		id: 'window-top-right',
		label: 'Move window to top right',
		hint: '',
		icon: 'maximize',
		desktopOnly: true,
		run: () => desktop('window_move', { position: 'top-right' }, () => undefined)
	},
	{
		id: 'window-bottom-left',
		label: 'Move window to bottom left',
		hint: '',
		icon: 'maximize',
		desktopOnly: true,
		run: () => desktop('window_move', { position: 'bottom-left' }, () => undefined)
	},
	{
		id: 'window-bottom-right',
		label: 'Move window to bottom right',
		hint: '',
		icon: 'maximize',
		desktopOnly: true,
		run: () => desktop('window_move', { position: 'bottom-right' }, () => undefined)
	},
	{
		id: 'open-log-folder',
		label: 'Open log folder',
		hint: '',
		icon: 'folder',
		run: () => openLogFolder()
	},
	{
		id: 'quit-atlas',
		label: 'Quit Atlas',
		hint: '',
		icon: 'x',
		desktopOnly: true,
		run: () => desktop('app_exit', undefined, () => undefined)
	}
];

/** A logical combo as the glyphs this platform spells it with, the same way `KeyHint`
 * draws it: joined tight on mac, with a plus everywhere else. */
function comboText(combo: string): string {
	const platform = shell.platform;
	return comboKeys(combo, platform).join(platform === 'mac' ? '' : '+');
}

/**
 * One palette command per contributed command. Running it sends the command id to every
 * mounted frame of that plugin; when none is mounted there is nobody to hear it, so the
 * run opens the plugin's first section instead, and says so when it has no section
 * either.
 *
 * A manifest's `combo` is printed as a hint, not bound: the shell's shortcut table is
 * fixed at build time and a plugin must not be able to take a key off the app.
 */
export function pluginCommands(contribs: Contributions): PaletteCommand[] {
	return contribs.commands.map((command) => {
		const section = contribs.sections.find((s) => s.pluginId === command.pluginId);
		// Printed as plain text rather than set as `combo`, which the palette renders as a
		// `KeyHint` and so would promise a key nothing listens for.
		const keys = command.combo ? comboText(command.combo) : '';
		return {
			id: `plugin:${command.pluginId}:${command.id}`,
			label: `${command.pluginName}: ${command.title}`,
			hint: keys ? `plugin command, ${keys} inside the plugin` : 'plugin command',
			icon: 'plug',
			run: (ctx: CommandContext) => {
				if (dispatchCommand(command.pluginId, command.id) > 0) return;
				if (section) return ctx.goto(sectionHref(section));
				push('info', `Open ${command.pluginName} first`);
			}
		};
	});
}

/** Case-insensitive substring over the label and the hint. Blank keeps them all.
 * `desktopOnly` commands are left out entirely outside Tauri, where invoking them would
 * only log a fallback warning and do nothing. `extra` is the contributed commands, passed
 * in rather than read here so that filtering stays a plain function of its arguments: the
 * palette reads the reactive plugin store where a read can be tracked, and this keeps
 * working from a test or anywhere else outside a component. */
export function filterCommands(text: string, extra: PaletteCommand[] = []): PaletteCommand[] {
	const available = [...COMMANDS, ...extra].filter((c) => !c.desktopOnly || inTauri());
	const needle = text.trim().toLowerCase();
	if (!needle) return available;
	return available.filter((c) => `${c.label} ${c.hint}`.toLowerCase().includes(needle));
}
