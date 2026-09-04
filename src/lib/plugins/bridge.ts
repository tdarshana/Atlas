// The host half of the plugin bridge: the postMessage protocol between the app and a
// plugin's sandboxed frame.
//
// Nothing here touches the DOM, so the whole protocol is testable against a fake
// `postMessage` target and a stub backend. The frame is sandboxed into an opaque origin,
// which means it can never be named as a `targetOrigin`; identity is checked the other
// way round instead, by comparing `event.source` against the frame's own window.

import type { PluginBackend } from './plugin-api';
import type { Permission, PluginInfo, Slot } from './types';

/** The API version the host implements, matching `ATLAS_API_VERSION` in Rust. */
export const ATLAS_API_VERSION = '1.0.0';

/** How short and how tall a frame may ask to be, in pixels. */
export const MIN_FRAME_HEIGHT = 40;
export const MAX_FRAME_HEIGHT = 2000;

export type ErrorCode = 'permission_denied' | 'unknown_method' | 'bad_params' | 'upstream';

export type NotifyKind = 'info' | 'success' | 'error';

const NOTIFY_KINDS: NotifyKind[] = ['info', 'success', 'error'];

/** Only `ui.` settings are readable, whatever permissions the manifest asks for. */
const READABLE_SETTING_PREFIX = 'ui.';

/**
 * The permission each method needs. A method absent from this table is
 * `unknown_method`; a `null` value is a method that needs no permission at all.
 */
const REQUIRED_PERMISSION: Record<string, Permission | null> = {
	'memories.search': 'memories.read',
	'memories.remember': 'memories.write',
	'tasks.list': 'tasks.read',
	'tasks.create': 'tasks.write',
	'tasks.move': 'tasks.write',
	'settings.get': 'settings.read',
	'ui.notify': null
};

/** Just enough of a `Window` to post to it, so a test can pass a plain object. */
export interface PostTarget {
	postMessage(message: unknown, targetOrigin: string): void;
}

/** The tokens handed to the frame, `--name` to value. */
export type ThemeTokens = Record<string, string>;

/**
 * What the surface around a frame is showing right now: `{ taskKey }` for a
 * `task.detail.panel`, and nothing at all for a frame whose surface has no subject. The
 * host sends it with `atlas:init` and again whenever it changes.
 */
export type FrameContext = Record<string, unknown>;

export interface BridgeOptions {
	plugin: PluginInfo;
	/** Which of the plugin's views the frame is showing. */
	view: string;
	/** The slot a contributed component renders in, when the frame is a component. */
	slot?: Slot | null;
	/** The frame's window. */
	target: PostTarget;
	/**
	 * What `event.source` must equal for a message to be handled. Defaults to `target`,
	 * which is the frame's own window in every real use.
	 */
	source?: unknown;
	api: PluginBackend;
	/** `plugin/<id>`; recorded here so the frame can be told who it is acting as. */
	actor: string;
	onResize?: (height: number) => void;
	onNotify?: (kind: NotifyKind, text: string) => void;
}

export interface Bridge {
	handle(event: { source?: unknown; data?: unknown }): void;
	sendInit(theme: ThemeTokens, context?: FrameContext): void;
	sendTheme(theme: ThemeTokens): void;
	sendContext(context: FrameContext): void;
	sendCommand(id: string): void;
	dispose(): void;
}

class BridgeError extends Error {
	constructor(
		public code: ErrorCode,
		message: string
	) {
		super(message);
	}
}

function str(value: unknown, field: string): string {
	if (typeof value !== 'string' || value === '') {
		throw new BridgeError('bad_params', `'${field}' must be a non-empty string.`);
	}
	return value;
}

function optionalStr(value: unknown, field: string): string | undefined {
	if (value === undefined || value === null) return undefined;
	return str(value, field);
}

function params(value: unknown): Record<string, unknown> {
	if (value === undefined || value === null) return {};
	if (typeof value !== 'object' || Array.isArray(value)) {
		throw new BridgeError('bad_params', "'params' must be an object.");
	}
	return value as Record<string, unknown>;
}

/** Clamps a frame's requested height into the range the host will honour. */
export function clampHeight(value: unknown): number | null {
	const height = typeof value === 'number' ? value : Number(value);
	if (!Number.isFinite(height)) return null;
	return Math.min(MAX_FRAME_HEIGHT, Math.max(MIN_FRAME_HEIGHT, Math.round(height)));
}

/**
 * Wires one frame to the app. `handle` is meant to be installed as a `message` listener
 * on the host window; everything it does not recognise it leaves alone, so several frames
 * can share the same window without stealing each other's messages.
 */
export function createBridge(options: BridgeOptions): Bridge {
	const { plugin, view, slot = null, target, api, actor, onResize, onNotify } = options;
	const source = options.source ?? target;
	const granted = new Set<Permission>(plugin.manifest?.permissions ?? []);
	let disposed = false;

	function post(message: unknown): void {
		if (disposed) return;
		target.postMessage(message, '*');
	}

	function need(permission: Permission): void {
		if (!granted.has(permission)) {
			throw new BridgeError('permission_denied', `This plugin does not hold '${permission}'.`);
		}
	}

	async function proxy(method: string, raw: unknown): Promise<unknown> {
		// `Object.hasOwn`, not a plain lookup: an object literal inherits `toString`,
		// `constructor`, `__proto__` and the rest, so `REQUIRED_PERMISSION[method]` would
		// answer with a function for those names and walk straight past the guard that is
		// meant to send them back as `unknown_method`.
		if (!Object.hasOwn(REQUIRED_PERMISSION, method)) {
			throw new BridgeError('unknown_method', `'${method}' is not a method the host offers.`);
		}
		const required = REQUIRED_PERMISSION[method];
		if (required !== null) need(required);
		const p = params(raw);

		switch (method) {
			case 'memories.search':
				return api.searchMemories(
					str(p.query, 'query'),
					typeof p.limit === 'number' ? p.limit : undefined
				);
			case 'memories.remember':
				return api.remember({
					text: str(p.text, 'text'),
					kind: optionalStr(p.kind, 'kind') as never,
					tags: Array.isArray(p.tags) ? (p.tags as string[]) : undefined
				});
			case 'tasks.list':
				return api.listTasks({
					project_id: optionalStr(p.project_id, 'project_id') ?? null,
					stage: optionalStr(p.stage, 'stage') ?? null,
					include_done: p.include_done === true
				});
			case 'tasks.create':
				return api.createTask({
					title: str(p.title, 'title'),
					description: optionalStr(p.description, 'description'),
					kind: optionalStr(p.kind, 'kind'),
					priority: optionalStr(p.priority, 'priority'),
					project_id: optionalStr(p.project_id, 'project_id') ?? null
				});
			case 'tasks.move':
				return api.moveTask(str(p.key, 'key'), str(p.stage, 'stage'));
			case 'settings.get': {
				const key = str(p.key, 'key');
				// The permission is not enough on its own: only the `ui.` settings are a
				// plugin's business, and the rest may hold a path or a credential.
				if (!key.startsWith(READABLE_SETTING_PREFIX)) {
					throw new BridgeError('permission_denied', `'${key}' is not a setting a plugin may read.`);
				}
				return api.getSetting(key);
			}
			case 'ui.notify': {
				const kind = str(p.kind, 'kind');
				if (!NOTIFY_KINDS.includes(kind as NotifyKind)) {
					throw new BridgeError('bad_params', `'${kind}' is not a notification kind.`);
				}
				const text = str(p.text, 'text');
				onNotify?.(kind as NotifyKind, text);
				return null;
			}
			default:
				throw new BridgeError('unknown_method', `'${method}' is not a method the host offers.`);
		}
	}

	async function answer(id: number, method: unknown, raw: unknown): Promise<void> {
		try {
			if (typeof method !== 'string') {
				throw new BridgeError('bad_params', "'method' must be a string.");
			}
			const result = await proxy(method, raw);
			post({ type: 'atlas:response', id, ok: true, result });
		} catch (e) {
			const code: ErrorCode = e instanceof BridgeError ? e.code : 'upstream';
			const message = e instanceof Error ? e.message : String(e);
			post({ type: 'atlas:response', id, ok: false, error: { code, message } });
		}
	}

	return {
		handle(event) {
			if (disposed || event.source !== source) return;
			const data = event.data;
			if (!data || typeof data !== 'object') return;
			const message = data as Record<string, unknown>;

			if (message.type === 'atlas:resize') {
				const height = clampHeight(message.height);
				if (height !== null) onResize?.(height);
				return;
			}
			if (message.type !== 'atlas:request') return;
			if (typeof message.id !== 'number') return;
			void answer(message.id, message.method, message.params);
		},
		sendInit(theme, context) {
			post({
				type: 'atlas:init',
				plugin: { id: plugin.id, view, slot },
				api: ATLAS_API_VERSION,
				actor,
				theme,
				context: context ?? {}
			});
		},
		sendTheme(theme) {
			post({ type: 'atlas:theme', theme });
		},
		sendContext(context) {
			post({ type: 'atlas:context', context });
		},
		sendCommand(id) {
			post({ type: 'atlas:command', id });
		},
		dispose() {
			disposed = true;
		}
	};
}
