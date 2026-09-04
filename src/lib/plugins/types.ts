// The plugin shapes the Rust side sends over `invoke`, mirroring
// `src-tauri/src/plugins/{manifest,registry}.rs`. The wire names are the dotted ones the
// manifest uses (`memories.read`, `dashboard.card`), not the Rust variant names.

export type Permission =
	| 'memories.read'
	| 'memories.write'
	| 'tasks.read'
	| 'tasks.write'
	| 'settings.read'
	| 'ui.sections'
	| 'ui.components'
	| 'mcp.tools';

export type Slot = 'dashboard.card' | 'board.card.badge' | 'task.detail.panel' | 'table.column';

/**
 * Where a frame is running. The four `Slot` values come from a manifest's
 * `contributes.components`; `background` is the host's own hidden frame, which fills no
 * slot on screen and exists so a plugin with MCP tools has a running context even when
 * none of its views is open.
 */
export type FrameSlot = Slot | 'background';

export interface Section {
	id: string;
	title: string;
	icon: string;
	/** The view name the frame is told to render; the route segment after the id. */
	view: string;
}

export interface ThemeContribution {
	id: string;
	name: string;
	/** A theme pack JSON file inside the plugin's folder, read through `plugin_read_file`
	 * and validated by `validateThemePack` exactly as an imported pack is. */
	file: string;
}

export interface Component {
	slot: Slot;
	id: string;
	view: string;
}

export interface CommandContribution {
	id: string;
	title: string;
	combo?: string;
}

export interface ToolContribution {
	name: string;
	description: string;
	args?: unknown;
	scope: string;
}

export interface Contributes {
	sections: Section[];
	themes: ThemeContribution[];
	components: Component[];
	commands: CommandContribution[];
	tools: ToolContribution[];
}

export interface Manifest {
	id: string;
	name: string;
	version: string;
	description: string;
	author: string;
	/** A semver range against the app's API version, e.g. `">=1.0 <2"`. */
	api: string;
	main: string;
	permissions: Permission[];
	contributes: Contributes;
}

/**
 * One installed plugin. `manifest` is null when `atlas-plugin.json` is missing or does not
 * parse; such a plugin is listed as incompatible with the reason and contributes nothing.
 */
export interface PluginInfo {
	id: string;
	manifest: Manifest | null;
	enabled: boolean;
	compatible: boolean;
	reason: string | null;
	dir: string;
}
