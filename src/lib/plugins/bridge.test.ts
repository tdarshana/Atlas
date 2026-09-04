// The bridge's rules: a method a plugin does not hold the permission for is refused
// before anything is proxied, ids come back matched even when two requests overlap,
// another window's messages are not this frame's, and `settings.get` is narrower than the
// permission that gates it.

import { describe, expect, it, vi } from 'vitest';
import type { Task } from '$lib/types';
import { BRIDGE_CLIENT_JS } from './bridge-client';
import {
	createBridge,
	MAX_FRAME_HEIGHT,
	MIN_FRAME_HEIGHT,
	TOOL_CALL_TIMEOUT_MS,
	type PostTarget
} from './bridge';
import type { PluginBackend } from './plugin-api';
import type { Manifest, Permission, PluginInfo } from './types';

interface Sent {
	type: string;
	id?: number;
	ok?: boolean;
	result?: unknown;
	error?: { code: string; message: string };
	[key: string]: unknown;
}

function harness(permissions: Permission[], granted?: Permission[]) {
	const sent: Sent[] = [];
	const target: PostTarget = { postMessage: (m) => void sent.push(m as Sent) };
	const manifest: Manifest = {
		id: 'hello-world',
		name: 'Hello World',
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions,
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [] }
	};
	const plugin: PluginInfo = {
		id: 'hello-world',
		manifest,
		enabled: true,
		compatible: true,
		reason: null,
		dir: '/plugins/hello-world',
		// The bridge gates on the grants, so an unrevoked fixture grants the whole manifest.
		granted: granted ?? [...permissions]
	};
	const api = {
		searchMemories: vi.fn(async () => [{ id: 'm1' }]),
		remember: vi.fn(async () => ({ id: 'm2' })),
		listTasks: vi.fn(async () => [{ key: 'ATL-1' }]),
		createTask: vi.fn(async () => ({ key: 'ATL-2' })),
		moveTask: vi.fn(async () => ({ key: 'ATL-1' })),
		getSetting: vi.fn(async () => 'dark')
	} as unknown as PluginBackend & Record<string, ReturnType<typeof vi.fn>>;
	const onResize = vi.fn();
	const onNotify = vi.fn();
	const bridge = createBridge({
		plugin,
		view: 'hello-view',
		target,
		api,
		actor: 'plugin/hello-world',
		onResize,
		onNotify
	});
	const request = (id: number, method: string, params?: unknown) =>
		bridge.handle({ source: target, data: { type: 'atlas:request', id, method, params } });
	return { sent, target, api, bridge, request, onResize, onNotify };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('createBridge', () => {
	it('refuses a method the manifest holds no permission for, and proxies nothing', async () => {
		const h = harness(['ui.sections']);
		h.request(1, 'memories.search', { query: 'anything' });
		await flush();

		expect(h.sent).toHaveLength(1);
		expect(h.sent[0]).toMatchObject({
			type: 'atlas:response',
			id: 1,
			ok: false,
			error: { code: 'permission_denied' }
		});
		expect(h.api.searchMemories).not.toHaveBeenCalled();
	});

	/** Revoking a permission on the Permissions view has to stop the call, not just change
	 * what the row says: the manifest still asks for `tasks.read`, but the grant is gone. */
	it('refuses a method whose permission the user revoked, even though the manifest asks for it', async () => {
		const h = harness(['tasks.read'], []);
		h.request(1, 'tasks.list', {});
		await flush();

		expect(h.sent[0]).toMatchObject({ id: 1, ok: false, error: { code: 'permission_denied' } });
		expect(h.api.listTasks).not.toHaveBeenCalled();
	});

	it('proxies a method the manifest does hold', async () => {
		const h = harness(['memories.read']);
		h.request(7, 'memories.search', { query: 'atlas', limit: 3 });
		await flush();

		expect(h.api.searchMemories).toHaveBeenCalledWith('atlas', 3);
		expect(h.sent[0]).toMatchObject({ id: 7, ok: true, result: [{ id: 'm1' }] });
	});

	it('answers an unknown method with unknown_method', async () => {
		const h = harness(['memories.read']);
		h.request(2, 'memories.forget', {});
		await flush();

		expect(h.sent[0]).toMatchObject({ id: 2, ok: false, error: { code: 'unknown_method' } });
	});

	it('matches each response id to its own request under two interleaved calls', async () => {
		const h = harness(['tasks.read', 'settings.read']);
		const tasks = [{ key: 'ATL-1' }] as unknown as Task[];
		let releaseTasks: (value: Task[]) => void = () => {};
		h.api.listTasks = vi.fn(() => new Promise<Task[]>((resolve) => (releaseTasks = resolve)));

		h.request(11, 'tasks.list', {});
		h.request(12, 'settings.get', { key: 'ui.theme' });
		await flush();

		// The second request answered first; its id must be its own.
		expect(h.sent).toHaveLength(1);
		expect(h.sent[0]).toMatchObject({ id: 12, ok: true, result: 'dark' });

		releaseTasks(tasks);
		await flush();
		expect(h.sent).toHaveLength(2);
		expect(h.sent[1]).toMatchObject({ id: 11, ok: true, result: [{ key: 'ATL-1' }] });
	});

	it('ignores a message from another window', async () => {
		const h = harness(['memories.read']);
		h.bridge.handle({
			source: { postMessage: () => {} },
			data: { type: 'atlas:request', id: 3, method: 'memories.search', params: { query: 'x' } }
		});
		await flush();

		expect(h.sent).toHaveLength(0);
		expect(h.api.searchMemories).not.toHaveBeenCalled();
	});

	it('denies settings.get for a key outside ui., even with the permission', async () => {
		const h = harness(['settings.read']);
		h.request(4, 'settings.get', { key: 'openrouter.api_key' });
		await flush();

		expect(h.sent[0]).toMatchObject({ id: 4, ok: false, error: { code: 'permission_denied' } });
		expect(h.api.getSetting).not.toHaveBeenCalled();

		h.request(5, 'settings.get', { key: 'ui.theme' });
		await flush();
		expect(h.sent[1]).toMatchObject({ id: 5, ok: true, result: 'dark' });
	});

	it('needs no permission for ui.notify, and checks the kind', async () => {
		const h = harness([]);
		h.request(6, 'ui.notify', { kind: 'success', text: 'Saved' });
		await flush();
		expect(h.onNotify).toHaveBeenCalledWith('success', 'Saved');

		h.request(8, 'ui.notify', { kind: 'shout', text: 'Saved' });
		await flush();
		expect(h.sent[1]).toMatchObject({ id: 8, ok: false, error: { code: 'bad_params' } });
	});

	it('clamps a resize into the range the host honours', () => {
		const h = harness([]);
		const resize = (height: unknown) =>
			h.bridge.handle({ source: h.target, data: { type: 'atlas:resize', height } });

		resize(320);
		resize(-5);
		resize(99999);
		resize('tall');

		expect(h.onResize.mock.calls.map((c) => c[0])).toEqual([320, MIN_FRAME_HEIGHT, MAX_FRAME_HEIGHT]);
	});

	it('reports a backend failure as upstream with the daemon message', async () => {
		const h = harness(['tasks.write']);
		h.api.createTask = vi.fn(async () => {
			throw new Error('Stage "Nope" is not on the board.');
		});
		h.request(9, 'tasks.create', { title: 'Try' });
		await flush();

		expect(h.sent[0]).toMatchObject({
			id: 9,
			ok: false,
			error: { code: 'upstream', message: 'Stage "Nope" is not on the board.' }
		});
	});

	it('sends init, theme and command messages the client understands', () => {
		const h = harness([]);
		h.bridge.sendInit({ '--accent': '#f00' });
		h.bridge.sendTheme({ '--accent': '#0f0' });
		h.bridge.sendCommand('hello.say');

		expect(h.sent[0]).toMatchObject({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'hello-view', slot: null },
			actor: 'plugin/hello-world'
		});
		expect(h.sent[1]).toMatchObject({ type: 'atlas:theme', theme: { '--accent': '#0f0' } });
		expect(h.sent[2]).toMatchObject({ type: 'atlas:command', id: 'hello.say' });
	});

	it('goes quiet once disposed', async () => {
		const h = harness(['memories.read']);
		h.bridge.dispose();
		h.request(10, 'memories.search', { query: 'x' });
		await flush();
		expect(h.sent).toHaveLength(0);
	});

	it('answers an inherited Object property name with unknown_method, not permission_denied', async () => {
		const h = harness(['memories.read', 'tasks.read']);
		// A plain lookup on an object literal resolves these through the prototype chain,
		// which would walk past the guard that is supposed to catch an unknown method.
		for (const [id, method] of [
			[20, 'toString'],
			[21, '__proto__'],
			[22, 'constructor'],
			[23, 'hasOwnProperty']
		] as const) {
			h.request(id, method, {});
		}
		await flush();

		expect(h.sent.map((m) => m.error?.code)).toEqual([
			'unknown_method',
			'unknown_method',
			'unknown_method',
			'unknown_method'
		]);
	});
});

/**
 * Runs the real bridge client, the same text Rust serves to the frame, against a fake
 * window so the two halves of the handshake are tested against each other rather than
 * against a hand-written imitation of the other side.
 */
function runClient() {
	const posted: Record<string, unknown>[] = [];
	const listeners: ((event: { data: unknown }) => void)[] = [];
	const applied: Record<string, string> = {};
	const win = {
		addEventListener: (type: string, cb: (event: { data: unknown }) => void) => {
			if (type === 'message') listeners.push(cb);
		}
	} as Record<string, unknown>;
	const parent = { postMessage: (m: unknown) => void posted.push(m as Record<string, unknown>) };
	const doc = {
		documentElement: {
			style: { setProperty: (name: string, value: string) => void (applied[name] = value) }
		}
	};

	new Function('window', 'parent', 'document', BRIDGE_CLIENT_JS)(win, parent, doc);

	return {
		posted,
		applied,
		atlas: win.atlas as {
			ready: Promise<{
				plugin: { id: string };
				theme: Record<string, string>;
				context: Record<string, unknown>;
			}>;
			request(method: string, params?: unknown): Promise<unknown>;
			resize(height: number): void;
			onContext(cb: (context: Record<string, unknown>) => void): void;
			onTool(name: string, handler: (args: unknown) => unknown): void;
		},
		deliver: (data: unknown) => listeners.forEach((cb) => cb({ data }))
	};
}

describe('the bridge client handshake', () => {
	it('says hello as soon as it runs, before anything else', () => {
		const client = runClient();
		expect(client.posted).toEqual([{ type: 'atlas:hello' }]);
	});

	it('buffers calls made before init and sends them once it arrives', async () => {
		const client = runClient();
		void client.atlas.request('tasks.list', {});
		client.atlas.resize(200);
		// Nothing but the hello has gone out: the host has not answered yet.
		expect(client.posted).toHaveLength(1);

		client.deliver({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'hello-view', slot: null },
			api: '1.0.0',
			theme: { '--accent': '#f00', '--color-scheme': 'dark' }
		});

		expect(client.posted.slice(1)).toEqual([
			{ type: 'atlas:request', id: 1, method: 'tasks.list', params: {} },
			{ type: 'atlas:resize', height: 200 }
		]);
		expect(client.applied).toEqual({ '--accent': '#f00', '--color-scheme': 'dark' });
		await expect(client.atlas.ready).resolves.toMatchObject({ plugin: { id: 'hello-world' } });
	});

	it('is answered by a host bridge bound to it on hello', async () => {
		const client = runClient();
		const h = harness(['tasks.read']);
		// What `PluginFrame` does when the hello lands: point a bridge at the frame and
		// send it the theme.
		const hostBridge = createBridge({
			plugin: {
				id: 'hello-world',
				manifest: {
					id: 'hello-world',
					name: 'Hello World',
					version: '1.0.0',
					description: 'd',
					author: 'a',
					api: '>=1.0 <2',
					main: 'main.js',
					permissions: ['tasks.read'],
					contributes: { sections: [], themes: [], components: [], commands: [], tools: [] }
				},
				enabled: true,
				compatible: true,
				reason: null,
				dir: '/plugins/hello-world',
				granted: ['tasks.read']
			},
			view: 'hello-view',
			target: { postMessage: (m) => client.deliver(m) },
			source: 'frame-window',
			api: h.api,
			actor: 'plugin/hello-world'
		});

		const answer = client.atlas.request('tasks.list', {});
		hostBridge.sendInit({ '--accent': '#0f0' });
		// The buffered request came out on init; hand it to the host as the frame would.
		const buffered = client.posted.at(-1);
		hostBridge.handle({ source: 'frame-window', data: buffered });
		await expect(answer).resolves.toEqual([{ key: 'ATL-1' }]);
	});

	it('carries the context in the ready payload and again on every change', async () => {
		const client = runClient();
		const seen: Record<string, unknown>[] = [];
		client.atlas.onContext((context) => void seen.push(context));

		client.deliver({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'task-panel', slot: 'task.detail.panel' },
			api: '1.0.0',
			theme: {},
			context: { taskKey: 'ATL-1' }
		});

		// The context at init arrives through `ready`, not through the callback.
		await expect(client.atlas.ready).resolves.toMatchObject({ context: { taskKey: 'ATL-1' } });
		expect(seen).toEqual([]);

		client.deliver({ type: 'atlas:context', context: { taskKey: 'ATL-2' } });
		client.deliver({ type: 'atlas:context', context: {} });

		expect(seen).toEqual([{ taskKey: 'ATL-2' }, {}]);
	});

	it('is sent the context by a host bridge, on init and on change', () => {
		const h = harness([]);
		h.bridge.sendInit({ '--accent': '#0f0' }, { taskKey: 'ATL-7' });
		h.bridge.sendContext({ taskKey: 'ATL-8' });

		expect(h.sent[0]).toMatchObject({ type: 'atlas:init', context: { taskKey: 'ATL-7' } });
		expect(h.sent[1]).toEqual({ type: 'atlas:context', context: { taskKey: 'ATL-8' } });
		// A frame with no subject is told so, rather than left to guess.
		h.bridge.sendInit({});
		expect(h.sent[2]).toMatchObject({ type: 'atlas:init', context: {} });
	});
});

// The MCP tool path: the daemon forwards a call to the host, the host asks the frame, the
// frame answers with the same id. Both halves are exercised here, the host's against a
// fake frame and the client's against the real script.
describe('a forwarded MCP tool call', () => {
	it('goes out as atlas:tool and resolves on the matching result', async () => {
		const h = harness(['mcp.tools']);

		const answer = h.bridge.callTool('ready_count', { of: 'tasks' });

		expect(h.sent).toEqual([
			{ type: 'atlas:tool', id: 1, name: 'ready_count', args: { of: 'tasks' } }
		]);
		h.bridge.handle({
			source: h.target,
			data: { type: 'atlas:tool-result', id: 1, ok: true, result: { count: 3 } }
		});
		await expect(answer).resolves.toEqual({ count: 3 });
	});

	it('rejects with the message the frame gave when the plugin fails', async () => {
		const h = harness(['mcp.tools']);

		const answer = h.bridge.callTool('ready_count', {});
		h.bridge.handle({
			source: h.target,
			data: { type: 'atlas:tool-result', id: 1, ok: false, error: 'the board is empty' }
		});

		await expect(answer).rejects.toThrow('the board is empty');
	});

	it('ignores a result whose id nothing is waiting on', async () => {
		const h = harness(['mcp.tools']);

		const answer = h.bridge.callTool('ready_count', {});
		h.bridge.handle({ source: h.target, data: { type: 'atlas:tool-result', id: 99, ok: true, result: 1 } });
		h.bridge.handle({ source: h.target, data: { type: 'atlas:tool-result', id: 1, ok: true, result: 2 } });

		await expect(answer).resolves.toBe(2);
	});

	it('gives up before the daemon does, naming the plugin that went quiet', async () => {
		vi.useFakeTimers();
		try {
			const h = harness(['mcp.tools']);
			const answer = h.bridge.callTool('ready_count', {});
			const settled = expect(answer).rejects.toThrow('plugin hello-world did not answer');

			// The daemon's own limit is 30 seconds, so this has to fire first.
			await vi.advanceTimersByTimeAsync(TOOL_CALL_TIMEOUT_MS);
			await settled;
		} finally {
			vi.useRealTimers();
		}
	});

	it('fails a call in flight when the frame goes away', async () => {
		const h = harness(['mcp.tools']);

		const answer = h.bridge.callTool('ready_count', {});
		h.bridge.dispose();

		await expect(answer).rejects.toThrow('plugin hello-world is not running');
	});

	it('is answered by the frame from its onTool handler', async () => {
		const client = runClient();
		client.atlas.onTool('ready_count', (args) => ({ count: 3, args }));
		client.deliver({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'background', slot: 'background' },
			api: '1.0.0',
			theme: {}
		});

		client.deliver({ type: 'atlas:tool', id: 7, name: 'ready_count', args: { of: 'tasks' } });
		await flush();

		expect(client.posted.at(-1)).toEqual({
			type: 'atlas:tool-result',
			id: 7,
			ok: true,
			result: { count: 3, args: { of: 'tasks' } }
		});
	});

	it('says so rather than going quiet when nothing handles the name', async () => {
		const client = runClient();
		client.deliver({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'background', slot: 'background' },
			api: '1.0.0',
			theme: {}
		});

		client.deliver({ type: 'atlas:tool', id: 4, name: 'nope', args: {} });
		await flush();

		expect(client.posted.at(-1)).toEqual({
			type: 'atlas:tool-result',
			id: 4,
			ok: false,
			error: 'tool nope is not handled'
		});
	});

	it('reports a handler that throws or rejects as a failed call', async () => {
		const client = runClient();
		client.atlas.onTool('throws', () => {
			throw new Error('no board');
		});
		client.atlas.onTool('rejects', () => Promise.reject(new Error('no daemon')));
		client.deliver({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'background', slot: 'background' },
			api: '1.0.0',
			theme: {}
		});

		client.deliver({ type: 'atlas:tool', id: 1, name: 'throws', args: {} });
		client.deliver({ type: 'atlas:tool', id: 2, name: 'rejects', args: {} });
		await flush();

		expect(client.posted.slice(-2)).toEqual([
			{ type: 'atlas:tool-result', id: 1, ok: false, error: 'no board' },
			{ type: 'atlas:tool-result', id: 2, ok: false, error: 'no daemon' }
		]);
	});
});
