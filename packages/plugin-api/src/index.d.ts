// Type declarations for an Atlas plugin: the manifest it ships, the `atlas` global the
// host defines inside its frame, and every request method with its parameters and its
// result.
//
// Nothing here is code. The package carries declarations only, so a plugin written in
// TypeScript can type-check against the host without pulling anything into its bundle.
// The host's own copies of these shapes live in `src/lib/plugins/types.ts` (the manifest)
// and `src/lib/plugins/bridge.ts` (the protocol); these must be kept in step with them.

// ---- the manifest, `atlas-plugin.json` ------------------------------------------------

/** What a plugin may ask for. A contribution without its permission is refused at
 * install: `sections` needs `ui.sections`, `components` needs `ui.components` and
 * `tools` needs `mcp.tools`. Themes and commands need none. */
export type Permission =
	| 'memories.read'
	| 'memories.write'
	| 'tasks.read'
	| 'tasks.write'
	| 'settings.read'
	| 'ui.sections'
	| 'ui.components'
	| 'mcp.tools';

/**
 * Where a contributed component renders. `dashboard.card` and `task.detail.panel` are
 * rendered today; `board.card.badge` and `table.column` are reserved and are not rendered
 * yet, so a plugin declaring one is accepted but nothing appears.
 */
export type Slot = 'dashboard.card' | 'board.card.badge' | 'task.detail.panel' | 'table.column';

export interface Section {
	id: string;
	title: string;
	/** A Lucide icon name, drawn in the rail. */
	icon: string;
	/** The view name this frame is opened with, and the route segment after the id. */
	view: string;
}

export interface ComponentContribution {
	slot: Slot;
	id: string;
	view: string;
}

export interface CommandContribution {
	id: string;
	title: string;
	/** A logical combo such as `Mod+H`, printed beside the command. Not bound: the app's
	 * shortcut table is fixed, so this documents a key the plugin binds for itself. */
	combo?: string;
}

export interface ThemeContribution {
	id: string;
	name: string;
	/** A theme pack JSON file inside the plugin's folder. */
	file: string;
}

export interface ToolContribution {
	name: string;
	description: string;
	args?: unknown;
	scope: string;
}

export interface Contributes {
	sections?: Section[];
	components?: ComponentContribution[];
	commands?: CommandContribution[];
	themes?: ThemeContribution[];
	tools?: ToolContribution[];
}

export interface Manifest {
	/** Lower-case letters, digits and dashes; the folder the plugin is installed into. */
	id: string;
	name: string;
	version: string;
	description: string;
	author: string;
	/** A semver range against the host's API version, e.g. `">=1.0 <2"`. */
	api: string;
	/** The entry point, relative to the plugin folder. */
	main: string;
	permissions: Permission[];
	contributes: Contributes;
}

/** A theme pack, the shape a `themes` contribution's file holds. `name` in the file is
 * ignored: the manifest's `name` is what the Theme select shows. */
export interface ThemePack {
	name: string;
	base: 'dark' | 'light';
	/** Colour tokens plus `--radius-sm` and `--radius-md`; anything else is refused. */
	tokens: Record<string, string>;
}

// ---- what the host sends -------------------------------------------------------------

/** Every theme token the app is drawing itself with, `--name` to a literal value. The
 * client also writes these onto the frame's own root element. */
export type ThemeTokens = Record<string, string>;

/** What the surface around the frame is showing. A `task.detail.panel` gets
 * `{ taskKey }`; a frame whose surface has no subject gets an empty object. */
export interface FrameContext {
	taskKey?: string;
	[key: string]: unknown;
}

/** What `atlas.ready` resolves to, once the host has answered the frame's hello. */
export interface AtlasReady {
	plugin: {
		id: string;
		/** Which view this frame was opened for, from the manifest. */
		view: string;
		/** The slot, when this frame is a contributed component; null for a section. */
		slot: Slot | null;
	};
	/** The host's API version, e.g. `"1.0.0"`. */
	api: string;
	theme: ThemeTokens;
	context: FrameContext;
}

// ---- requests ------------------------------------------------------------------------

export type MemoryKind = 'fact' | 'decision' | 'preference' | 'insight' | 'todo';
export type MemoryStatus = 'active' | 'pending' | 'rejected' | 'superseded';
export type MemoryScope = 'global' | 'project';
export type TaskKind = 'task' | 'bug' | 'feature' | 'chore';
export type TaskPriority = 'low' | 'medium' | 'high' | 'urgent';

export interface Memory {
	id: string;
	scope: MemoryScope;
	project_id: string | null;
	kind: MemoryKind;
	text: string;
	tags: string[];
	source_agent: string | null;
	source_tool: string | null;
	confidence: number;
	status: MemoryStatus;
	superseded_by: string | null;
	created_at: string;
	updated_at: string;
}

export interface Task {
	id: string;
	/** The human handle, e.g. `ATL-12`. */
	key: string;
	project_id: string | null;
	seq: number;
	title: string;
	description: string;
	stage: string;
	kind: TaskKind;
	priority: TaskPriority;
	assignee: string | null;
	labels: string[];
	parent_id: string | null;
	created_by: string;
	created_at: string;
	updated_at: string;
	closed_at: string | null;
	/** Keys of the tasks this one waits on. */
	blocked_by: string[];
	/** How many of `blocked_by` are not themselves done. */
	open_blockers: number;
	/** Whether the task is free to be picked up. */
	ready: boolean;
	blocked_reason: string | null;
	subtasks_total: number;
	subtasks_done: number;
}

/** The parameters each method takes, and the permission it needs.
 *
 * | method              | permission        |
 * | ------------------- | ----------------- |
 * | `memories.search`   | `memories.read`   |
 * | `memories.remember` | `memories.write`  |
 * | `tasks.list`        | `tasks.read`      |
 * | `tasks.create`      | `tasks.write`     |
 * | `tasks.move`        | `tasks.write`     |
 * | `settings.get`      | `settings.read`   |
 * | `ui.notify`         | none              |
 */
export interface AtlasRequests {
	'memories.search': {
		params: { query: string; limit?: number };
		result: unknown;
	};
	'memories.remember': {
		/** Scope is always global and the kind defaults to `insight`: a plugin has no
		 * project of its own. */
		params: { text: string; kind?: MemoryKind; tags?: string[] };
		result: Memory;
	};
	'tasks.list': {
		params: { project_id?: string | null; stage?: string | null; include_done?: boolean };
		result: Task[];
	};
	'tasks.create': {
		params: {
			title: string;
			description?: string;
			kind?: string;
			priority?: string;
			project_id?: string | null;
		};
		result: Task;
	};
	'tasks.move': {
		params: { key: string; stage: string };
		result: Task;
	};
	'settings.get': {
		/** Only `ui.` keys are readable, whatever the manifest asks for. */
		params: { key: string };
		result: unknown;
	};
	'ui.notify': {
		params: { kind: NotifyKind; text: string };
		result: null;
	};
}

export type AtlasMethod = keyof AtlasRequests;

export type NotifyKind = 'info' | 'success' | 'error';

/** Why a request was refused.
 *
 * - `permission_denied`: the manifest does not hold the permission the method needs, or
 *   the key is one no plugin may read.
 * - `unknown_method`: no such method.
 * - `bad_params`: a parameter is missing or the wrong type.
 * - `upstream`: the daemon refused or could not be reached.
 */
export type ErrorCode = 'permission_denied' | 'unknown_method' | 'bad_params' | 'upstream';

/** What a rejected `atlas.request` throws: an `Error` carrying the code as well. */
export interface AtlasError extends Error {
	code: ErrorCode;
}

// ---- the client ----------------------------------------------------------------------

export interface AtlasClient {
	/** Resolves once the host has answered the frame's hello. Calls made before then are
	 * buffered, so a plugin need not wait to issue them. */
	readonly ready: Promise<AtlasReady>;
	request<M extends AtlasMethod>(
		method: M,
		params?: AtlasRequests[M]['params']
	): Promise<AtlasRequests[M]['result']>;
	/** Called on every theme change after init. The tokens are already on the frame's own
	 * root element by the time this runs. */
	onTheme(cb: (theme: ThemeTokens) => void): void;
	/** Called with the id of a contributed command the user ran from the palette. */
	onCommand(cb: (commandId: string) => void): void;
	/** Called on every context change after init; the context at init is in `ready`. */
	onContext(cb: (context: FrameContext) => void): void;
	/**
	 * Registers the handler for one of the manifest's `contributes.tools`, by its declared
	 * name. The handler is given the call's arguments and answers with a value or a promise
	 * for one; throwing or rejecting reports the message as the tool's error. Answer inside
	 * 25 seconds, after which the host gives up on the plugin's behalf.
	 */
	onTool(name: string, handler: (args: Record<string, unknown>) => unknown): void;
	/** Asks the host for a height in pixels, clamped to 40..2000 and to whatever ceiling
	 * the slot sets. Ignored for a section, which fills the content panel. */
	resize(height: number): void;
	/** Raises a toast in the app, prefixed with the plugin's name. */
	notify(kind: NotifyKind, text: string): Promise<null>;
}

declare global {
	// eslint-disable-next-line no-var
	var atlas: AtlasClient;

	interface Window {
		atlas: AtlasClient;
	}
}
