// The app's half of the plugin MCP tool path.
//
// The daemon owns the MCP server, so a plugin's tool is registered with the daemon over
// HTTP and then called back over one loopback WebSocket: the daemon sends
// `{ id, plugin_id, tool, args }` and waits at most 30 seconds for
// `{ id, ok, result | error }` carrying the same id. When that socket closes the daemon
// drops every registration it holds, so the app re-registers on each open.
//
// The socket itself is a parameter (`socketFactory`), which keeps the channel's logic
// free of the network: a test passes a fake and drives open, message and close by hand.

import { baseUrl } from '$lib/daemon.svelte';
import { activePlugins } from './contributions';
import { holds } from './grants';
import type { PluginInfo, ToolContribution } from './types';

/** One tool as the daemon's `PUT` body carries it. `plugin_id` is not sent: the path
 * names the plugin, and the daemon fills the field in, so a body cannot register under
 * another plugin's id. */
export interface ToolDecl {
	name: string;
	description: string;
	args: unknown;
	scope: string;
}

/** One plugin's whole set, which is what a `PUT` replaces. */
export interface PluginTools {
	pluginId: string;
	tools: ToolDecl[];
}

/** The daemon's frame, one per forwarded call. */
interface CallFrame {
	id: number;
	plugin_id: string;
	tool: string;
	args: unknown;
}

/** Just enough of a `WebSocket` to run the channel, so a test can pass a plain object. */
export interface ToolSocket {
	send(data: string): void;
	close(): void;
	onopen: (() => void) | null;
	onmessage: ((event: { data: unknown }) => void) | null;
	onclose: (() => void) | null;
	onerror: ((error?: unknown) => void) | null;
}

export interface ToolChannelOptions {
	url: string;
	socketFactory(url: string): ToolSocket;
	/** The tool-contributing plugins right now, read afresh on every open and `sync`. */
	registry(): PluginTools[];
	/** Replaces one plugin's registered set with `tools`. */
	register(pluginId: string, tools: ToolDecl[]): Promise<void>;
	/** Drops one plugin's whole set. */
	unregister(pluginId: string): Promise<void>;
	/** Runs one tool and answers with its result, or rejects with the reason. */
	onCall(pluginId: string, tool: string, args: unknown): Promise<unknown>;
	/** Where a failure that has nowhere else to go is reported. Defaults to `console.warn`. */
	onError?(message: string): void;
	/**
	 * A registration the daemon refused, which is a different thing from a socket that
	 * dropped: the plugin is installed and running, and its tools will stay missing until
	 * its author fixes the manifest. Defaults to `onError`.
	 */
	onRegisterError?(pluginId: string, message: string): void;
	/** Called on every open and every close, so a surface can say the channel is down. */
	onStatus?(connected: boolean): void;
}

export interface ToolChannel {
	/** Pushes the current `registry()` up: a `PUT` per plugin, and a `DELETE` for every
	 * plugin that was registered before and is not any more. A no-op while the socket is
	 * down, since the next open registers everything from scratch. */
	sync(): Promise<void>;
	/** True while the socket is open, for a caller that wants to know. */
	connected(): boolean;
	/** Closes the socket and stops reconnecting. */
	dispose(): void;
}

/** The backoff between reconnects, in milliseconds: 1s, 2s, 4s, ... capped at 30s. */
const FIRST_RETRY_MS = 1000;
const MAX_RETRY_MS = 30_000;

/** The channel's own URL, from the daemon's HTTP base. */
export function channelUrl(base: string = baseUrl()): string {
	return `${base.replace(/^http/, 'ws')}/api/v1/mcp/plugin-channel`;
}

/** The decls one plugin registers: its manifest's `contributes.tools`, with the schema a
 * tool omitted filled in as an object taking no arguments, which is what the daemon's
 * validation asks for. */
export function toolDecls(tools: ToolContribution[]): ToolDecl[] {
	return tools.map((tool) => ({
		name: tool.name,
		description: tool.description,
		args: tool.args ?? { type: 'object', properties: {}, additionalProperties: false },
		scope: tool.scope
	}));
}

/**
 * The plugins that contribute MCP tools right now: enabled, compatible, holding
 * `mcp.tools` and declaring at least one. The check is on the grant, not the manifest's
 * request: revoking `mcp.tools` on the Permissions view has to unregister the plugin's
 * tools, which is the whole point of a revocable grant.
 */
export function toolPlugins(items: PluginInfo[]): PluginInfo[] {
	return activePlugins(items).filter((p) => {
		const tools = p.manifest?.contributes.tools ?? [];
		if (tools.length === 0) return false;
		if (!holds(p, 'mcp.tools')) {
			console.warn(`plugin ${p.id} declares MCP tools without a held 'mcp.tools' permission`);
			return false;
		}
		return true;
	});
}

/** What `createToolChannel`'s `registry` answers with, from the plugin list. */
export function toolRegistry(items: PluginInfo[]): PluginTools[] {
	return toolPlugins(items).map((p) => ({
		pluginId: p.id,
		tools: toolDecls(p.manifest?.contributes.tools ?? [])
	}));
}

const JSON_HEADERS = { 'Content-Type': 'application/json', 'X-Atlas-Actor': 'desktop' };

/**
 * The daemon's own words for a refusal. It answers a malformed decl with
 * `400 {"error": ...}`, and that sentence is the only thing that says which rule the
 * manifest broke, so it is what a plugin's author needs to read.
 */
async function refusal(res: Response): Promise<string> {
	try {
		const body = (await res.json()) as { error?: unknown };
		if (typeof body?.error === 'string' && body.error !== '') return body.error;
	} catch {
		// Not JSON, so the status is all there is.
	}
	return `the daemon answered ${res.status}`;
}

/** `PUT /api/v1/mcp/plugin-tools/{id}`: replaces that plugin's whole set. */
export async function putPluginTools(pluginId: string, tools: ToolDecl[]): Promise<void> {
	const res = await fetch(`${baseUrl()}/api/v1/mcp/plugin-tools/${encodeURIComponent(pluginId)}`, {
		method: 'PUT',
		headers: JSON_HEADERS,
		body: JSON.stringify({ tools })
	});
	if (!res.ok) throw new Error(await refusal(res));
}

/** `DELETE /api/v1/mcp/plugin-tools/{id}`: drops that plugin's whole set. */
export async function deletePluginTools(pluginId: string): Promise<void> {
	const res = await fetch(`${baseUrl()}/api/v1/mcp/plugin-tools/${encodeURIComponent(pluginId)}`, {
		method: 'DELETE',
		headers: JSON_HEADERS
	});
	if (!res.ok) throw new Error(await refusal(res));
}

export function createToolChannel(options: ToolChannelOptions): ToolChannel {
	const { url, socketFactory, registry, register, unregister, onCall } = options;
	const report = options.onError ?? ((message: string) => console.warn(message));
	const reportRegister =
		options.onRegisterError ?? ((pluginId: string, message: string) => report(`${pluginId}: ${message}`));

	let socket: ToolSocket | null = null;
	let open = false;
	let disposed = false;
	let retry = FIRST_RETRY_MS;
	let timer: ReturnType<typeof setTimeout> | null = null;
	/** The plugin ids the daemon holds for us, so a plugin that lost its tools can be
	 * told apart from one that never had any. Cleared on close: the daemon drops the lot. */
	let registered = new Set<string>();
	/**
	 * The syncs run one after another. Two overlapping ones could otherwise order a `PUT`
	 * for a plugin after a `DELETE` for the same plugin, leaving the daemon advertising a
	 * tool the app has no frame for until something else happened to sync again.
	 */
	let queue: Promise<void> = Promise.resolve();

	async function runSync(): Promise<void> {
		if (!open) return;
		const current = registry();
		const ids = new Set(current.map((entry) => entry.pluginId));
		for (const entry of current) {
			try {
				await register(entry.pluginId, entry.tools);
				registered.add(entry.pluginId);
			} catch (e) {
				reportRegister(entry.pluginId, e instanceof Error ? e.message : String(e));
			}
		}
		for (const id of [...registered]) {
			if (ids.has(id)) continue;
			try {
				await unregister(id);
			} catch (e) {
				reportRegister(id, e instanceof Error ? e.message : String(e));
			}
			registered.delete(id);
		}
	}

	function sync(): Promise<void> {
		// The `catch` keeps the chain alive: a rejection left on `queue` would make every
		// later `sync` skip its turn.
		queue = queue.then(runSync).catch((e) => report(e instanceof Error ? e.message : String(e)));
		return queue;
	}

	function answer(frame: CallFrame): void {
		onCall(frame.plugin_id, frame.tool, frame.args).then(
			(result) => send({ id: frame.id, ok: true, result: result ?? null }),
			(error) => send({ id: frame.id, ok: false, error: error instanceof Error ? error.message : String(error) })
		);
	}

	function send(message: unknown): void {
		if (!socket || !open) return;
		socket.send(JSON.stringify(message));
	}

	function receive(data: unknown): void {
		if (typeof data !== 'string') return;
		let frame: CallFrame;
		try {
			frame = JSON.parse(data) as CallFrame;
		} catch {
			report('the plugin channel sent a frame that is not JSON');
			return;
		}
		if (typeof frame?.id !== 'number' || typeof frame.plugin_id !== 'string' || typeof frame.tool !== 'string') {
			report('the plugin channel sent a frame with no call in it');
			return;
		}
		answer(frame);
	}

	function scheduleReconnect(): void {
		if (disposed || timer !== null) return;
		const wait = retry;
		retry = Math.min(MAX_RETRY_MS, retry * 2);
		timer = setTimeout(() => {
			timer = null;
			connect();
		}, wait);
	}

	function connect(): void {
		if (disposed) return;
		socket = socketFactory(url);
		socket.onopen = () => {
			open = true;
			retry = FIRST_RETRY_MS;
			// The daemon cleared everything when the last socket went; start from nothing
			// so `sync` registers the whole current set rather than diffing against a
			// registry the daemon no longer holds.
			registered = new Set();
			options.onStatus?.(true);
			void sync();
		};
		socket.onmessage = (event) => receive(event.data);
		socket.onclose = () => {
			open = false;
			socket = null;
			options.onStatus?.(false);
			scheduleReconnect();
		};
		socket.onerror = () => {
			// A socket that errors always closes too, so the reconnect is left to `onclose`.
			report('the plugin channel reported an error');
		};
	}

	connect();

	return {
		sync,
		connected: () => open,
		dispose() {
			disposed = true;
			if (timer !== null) clearTimeout(timer);
			timer = null;
			open = false;
			socket?.close();
			socket = null;
		}
	};
}
