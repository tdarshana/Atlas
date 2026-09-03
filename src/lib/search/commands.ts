// The palette's command list. A command either navigates (the target page reads the
// query parameter and opens the right thing) or drives the shell directly. Only the
// combos the shell actually binds are printed, so a hint never promises a key that
// does nothing.

import type { IconName } from '$lib/ds';
import { setTheme, shell, toggleRail, toggleSidePanel } from '$lib/shell/shell.svelte';
import type { Uuid } from '$lib/types';

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
	}
];

/** Case-insensitive substring over the label and the hint. Blank keeps them all. */
export function filterCommands(text: string): PaletteCommand[] {
	const needle = text.trim().toLowerCase();
	if (!needle) return COMMANDS;
	return COMMANDS.filter((c) => `${c.label} ${c.hint}`.toLowerCase().includes(needle));
}
