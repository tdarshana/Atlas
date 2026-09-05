// The daemon's plugin tool channel, driven against a fake socket. The channel never
// touches the network here: `socketFactory` hands back an object a test opens, feeds and
// closes by hand, which is the whole reason the socket is a parameter.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	channelUrl,
	createToolChannel,
	deletePluginTools,
	putPluginTools,
	toolDecls,
	toolPlugins,
	toolRegistry,
	type PluginTools,
	type ToolDecl,
	type ToolChannel,
	type ToolSocket
} from './tools';
import type { Manifest, Permission, PluginInfo, ToolContribution } from './types';

vi.mock('$lib/daemon.svelte', () => ({
	baseUrl: () => 'http://127.0.0.1:7433',
	tokenQuery: (token?: string) => (token ? `?token=${token}` : '')
}));

const READY_COUNT: ToolContribution = {
	name: 'ready_count',
	description: 'Count the ready tasks on the board',
	args: { type: 'object', properties: {}, additionalProperties: false },
	scope: 'read'
};

function plugin(
	id: string,
	tools: ToolContribution[],
	permissions: Permission[] = ['mcp.tools'],
	overrides: Partial<PluginInfo> = {}
): PluginInfo {
	const manifest: Manifest = {
		id,
		name: id,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions,
		contributes: { sections: [], themes: [], components: [], commands: [], tools }
	};
	return {
		id,
		manifest,
		enabled: true,
		compatible: true,
		reason: null,
		dir: `/p/${id}`,
		granted: [...permissions],
		...overrides
	};
}

/** A socket the test opens, feeds and closes itself. */
function fakeSocket() {
	const sent: string[] = [];
	let closed = false;
	const socket: ToolSocket = {
		send: (data) => void sent.push(data),
		close: () => void (closed = true),
		onopen: null,
		onmessage: null,
		onclose: null,
		onerror: null
	};
	return {
		socket,
		sent,
		isClosed: () => closed,
		open: () => socket.onopen?.(),
		deliver: (frame: unknown) => socket.onmessage?.({ data: JSON.stringify(frame) }),
		drop: () => socket.onclose?.(),
		frames: () => sent.map((s) => JSON.parse(s) as Record<string, unknown>)
	};
}

function channelHarness(registry: () => PluginTools[]) {
	const sockets: ReturnType<typeof fakeSocket>[] = [];
	const register = vi.fn(async (_pluginId: string, _tools: ToolDecl[]) => {});
	const unregister = vi.fn(async (_pluginId: string) => {});
	const onCall = vi.fn(async () => ({ count: 3 }));
	const onError = vi.fn();
	const onRegisterError = vi.fn();
	const onStatus = vi.fn();
	const channel = createToolChannel({
		url: 'ws://127.0.0.1:7433/api/v1/mcp/plugin-channel',
		socketFactory: () => {
			const s = fakeSocket();
			sockets.push(s);
			return s.socket;
		},
		registry,
		register,
		unregister,
		onCall,
		onError,
		onRegisterError,
		onStatus
	});
	return {
		channel,
		sockets,
		register,
		unregister,
		onCall,
		onError,
		onRegisterError,
		onStatus,
		latest: () => sockets[sockets.length - 1]
	};
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

let channels: ToolChannel[] = [];

beforeEach(() => {
	channels = [];
});

afterEach(() => {
	for (const c of channels) c.dispose();
	vi.useRealTimers();
});

describe('channelUrl', () => {
	it('is the daemon base with the ws scheme and the channel path', () => {
		expect(channelUrl('http://127.0.0.1:7433')).toBe('ws://127.0.0.1:7433/api/v1/mcp/plugin-channel');
	});

	// SEC-5: a WebSocket cannot carry a header, so the daemon token rides in the query.
	it('carries the daemon token as a query parameter', () => {
		expect(channelUrl('http://127.0.0.1:7433', 'secret')).toBe(
			'ws://127.0.0.1:7433/api/v1/mcp/plugin-channel?token=secret'
		);
	});
});

describe('toolDecls and toolPlugins', () => {
	it('fills in a schema for a tool that declared none', () => {
		expect(toolDecls([{ name: 'n', description: 'd', scope: 'read' }])).toEqual([
			{
				name: 'n',
				description: 'd',
				args: { type: 'object', properties: {}, additionalProperties: false },
				scope: 'read'
			}
		]);
	});

	it('takes only enabled, compatible plugins that declare a tool and hold mcp.tools', () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		const items = [
			plugin('with-tools', [READY_COUNT]),
			plugin('no-tools', []),
			plugin('no-permission', [READY_COUNT], ['tasks.read']),
			plugin('disabled', [READY_COUNT], ['mcp.tools'], { enabled: false }),
			plugin('incompatible', [READY_COUNT], ['mcp.tools'], { compatible: false })
		];

		expect(toolPlugins(items).map((p) => p.id)).toEqual(['with-tools']);
		expect(warn).toHaveBeenCalledWith(
			"plugin no-permission declares MCP tools without a held 'mcp.tools' permission"
		);
		warn.mockRestore();
	});

	it('drops a plugin whose mcp.tools grant was revoked, manifest or not', () => {
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		// The manifest still asks for `mcp.tools`; the user took it back on the Permissions
		// view. Revoking it has to unregister the plugin's tools, which is the whole point
		// of a revocable grant.
		const revoked = plugin('revoked', [READY_COUNT], ['mcp.tools'], { granted: [] });

		expect(toolPlugins([revoked])).toEqual([]);
		expect(warn).toHaveBeenCalledWith(
			"plugin revoked declares MCP tools without a held 'mcp.tools' permission"
		);
		warn.mockRestore();
	});

	it('answers with one entry per plugin, carrying its whole set', () => {
		expect(toolRegistry([plugin('hello-world', [READY_COUNT]), plugin('quiet', [])])).toEqual([
			{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) }
		]);
	});
});

describe('createToolChannel', () => {
	it('registers every plugin as soon as the socket opens', async () => {
		const h = channelHarness(() => [{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) }]);
		channels.push(h.channel);

		// Nothing goes out before the socket is up: the daemon holds no registry then.
		expect(h.register).not.toHaveBeenCalled();

		h.latest().open();
		await flush();

		expect(h.register).toHaveBeenCalledWith('hello-world', toolDecls([READY_COUNT]));
		expect(h.channel.connected()).toBe(true);
	});

	it('answers a forwarded call with the matching id', async () => {
		const h = channelHarness(() => []);
		channels.push(h.channel);
		h.latest().open();

		h.latest().deliver({ id: 7, plugin_id: 'hello-world', tool: 'ready_count', args: { of: 'tasks' } });
		await flush();

		expect(h.onCall).toHaveBeenCalledWith('hello-world', 'ready_count', { of: 'tasks' });
		expect(h.latest().frames()).toEqual([{ id: 7, ok: true, result: { count: 3 } }]);
	});

	it('maps a rejection to a failure frame carrying the reason', async () => {
		const h = channelHarness(() => []);
		channels.push(h.channel);
		h.onCall.mockRejectedValueOnce(new Error('plugin hello-world is not running'));
		h.latest().open();

		h.latest().deliver({ id: 8, plugin_id: 'hello-world', tool: 'ready_count', args: {} });
		await flush();

		expect(h.latest().frames()).toEqual([
			{ id: 8, ok: false, error: 'plugin hello-world is not running' }
		]);
	});

	it('reconnects with a growing backoff and registers again on each open', async () => {
		vi.useFakeTimers();
		const h = channelHarness(() => [{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) }]);
		channels.push(h.channel);
		h.latest().open();
		await vi.advanceTimersByTimeAsync(0);
		expect(h.register).toHaveBeenCalledTimes(1);

		h.latest().drop();
		// Nothing yet: the first retry is a second away.
		await vi.advanceTimersByTimeAsync(999);
		expect(h.sockets).toHaveLength(1);
		await vi.advanceTimersByTimeAsync(1);
		expect(h.sockets).toHaveLength(2);

		// A socket that opens and drops again waits twice as long.
		h.latest().open();
		await vi.advanceTimersByTimeAsync(0);
		expect(h.register).toHaveBeenCalledTimes(2);
		h.latest().drop();
		await vi.advanceTimersByTimeAsync(999);
		expect(h.sockets).toHaveLength(2);
		await vi.advanceTimersByTimeAsync(1);
		expect(h.sockets).toHaveLength(3);

		// The third socket never opens, so nothing resets the backoff and the next wait is
		// twice the last one.
		h.latest().drop();
		await vi.advanceTimersByTimeAsync(1999);
		expect(h.sockets).toHaveLength(3);
		await vi.advanceTimersByTimeAsync(1);
		expect(h.sockets).toHaveLength(4);
	});

	it('drops the plugins that lost their tools on the next sync', async () => {
		let current: PluginTools[] = [
			{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) },
			{ pluginId: 'second', tools: toolDecls([READY_COUNT]) }
		];
		const h = channelHarness(() => current);
		channels.push(h.channel);
		h.latest().open();
		await flush();
		expect(h.register).toHaveBeenCalledTimes(2);

		current = [{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) }];
		await h.channel.sync();

		expect(h.unregister).toHaveBeenCalledWith('second');
		expect(h.unregister).toHaveBeenCalledTimes(1);
	});

	it('stops reconnecting once disposed', async () => {
		vi.useFakeTimers();
		const h = channelHarness(() => []);
		h.latest().open();
		h.channel.dispose();

		expect(h.latest().isClosed()).toBe(true);
		expect(h.channel.connected()).toBe(false);

		await vi.advanceTimersByTimeAsync(60_000);
		expect(h.sockets).toHaveLength(1);
	});

	it('says so rather than throwing when a frame is not a call', async () => {
		const h = channelHarness(() => []);
		channels.push(h.channel);
		h.latest().open();

		h.latest().socket.onmessage?.({ data: 'not json' });
		h.latest().deliver({ hello: true });
		await flush();

		expect(h.onError).toHaveBeenCalledTimes(2);
		expect(h.onCall).not.toHaveBeenCalled();
		expect(h.latest().frames()).toEqual([]);
	});
});

describe('createToolChannel, after the first review', () => {
	it('caps the reconnect backoff at 30 seconds', async () => {
		vi.useFakeTimers();
		const h = channelHarness(() => []);
		channels.push(h.channel);

		// 1s, 2s, 4s, 8s, 16s, then 30s twice: seven drops with no open in between, so
		// nothing ever resets the backoff.
		const waits = [1000, 2000, 4000, 8000, 16_000, 30_000, 30_000];
		for (const [index, wait] of waits.entries()) {
			h.latest().drop();
			await vi.advanceTimersByTimeAsync(wait - 1);
			expect(h.sockets).toHaveLength(index + 1);
			await vi.advanceTimersByTimeAsync(1);
			expect(h.sockets).toHaveLength(index + 2);
		}
	});

	it('reports the socket coming up and going down', async () => {
		const h = channelHarness(() => []);
		channels.push(h.channel);

		h.latest().open();
		await flush();
		h.latest().drop();

		expect(h.onStatus.mock.calls.map((c) => c[0])).toEqual([true, false]);
	});

	it('reports a refused registration separately, naming the plugin', async () => {
		const h = channelHarness(() => [{ pluginId: 'hello-world', tools: toolDecls([READY_COUNT]) }]);
		channels.push(h.channel);
		h.register.mockRejectedValueOnce(new Error("tool name 'a__b' may not contain '__'."));

		h.latest().open();
		await flush();

		expect(h.onRegisterError).toHaveBeenCalledWith(
			'hello-world',
			"tool name 'a__b' may not contain '__'."
		);
		// A refusal is not a transport failure, so the quiet channel of last resort is unused.
		expect(h.onError).not.toHaveBeenCalled();
	});

	it('runs overlapping syncs one after another rather than interleaving them', async () => {
		const order: string[] = [];
		const h = channelHarness(() => [
			{ pluginId: 'a', tools: toolDecls([READY_COUNT]) },
			{ pluginId: 'b', tools: toolDecls([READY_COUNT]) }
		]);
		channels.push(h.channel);
		// A register that yields halfway, so an unserialised second sync would start its
		// own `a` before this one finished.
		h.register.mockImplementation(async (id: string) => {
			order.push(`start ${id}`);
			await flush();
			order.push(`end ${id}`);
		});

		h.latest().open();
		await Promise.all([h.channel.sync(), h.channel.sync()]);

		// Three syncs of two plugins each, and every start is closed by its own end before
		// the next start.
		expect(order).toHaveLength(12);
		for (let i = 0; i < order.length; i += 2) {
			expect(order[i + 1]).toBe(order[i].replace('start', 'end'));
		}
	});
});

describe('putPluginTools and deletePluginTools', () => {
	interface Call {
		url: string;
		method?: string;
		headers?: Record<string, string>;
		body?: string;
	}

	function fakeFetch(response: { ok: boolean; status?: number; body?: unknown }) {
		const calls: Call[] = [];
		const fn = vi.fn(async (url: string, init: Record<string, unknown>) => {
			calls.push({ url, ...(init as object) } as Call);
			return {
				ok: response.ok,
				status: response.status ?? (response.ok ? 204 : 400),
				json: async () => {
					if (response.body === undefined) throw new Error('not JSON');
					return response.body;
				}
			} as unknown as Response;
		});
		vi.stubGlobal('fetch', fn);
		return calls;
	}

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('PUTs the whole set to the plugin path, as the desktop actor', async () => {
		const calls = fakeFetch({ ok: true });

		await putPluginTools('hello-world', toolDecls([READY_COUNT]));

		expect(calls).toHaveLength(1);
		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/mcp/plugin-tools/hello-world');
		expect(calls[0].method).toBe('PUT');
		expect(calls[0].headers).toEqual({
			'Content-Type': 'application/json',
			'X-Atlas-Actor': 'desktop'
		});
		expect(JSON.parse(calls[0].body ?? '')).toEqual({ tools: toolDecls([READY_COUNT]) });
	});

	it('DELETEs the whole set from the same path, with no body', async () => {
		const calls = fakeFetch({ ok: true });

		await deletePluginTools('hello-world');

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/mcp/plugin-tools/hello-world');
		expect(calls[0].method).toBe('DELETE');
		expect(calls[0].headers?.['X-Atlas-Actor']).toBe('desktop');
		expect(calls[0].body).toBeUndefined();
	});

	it('escapes the plugin id in the path', async () => {
		const calls = fakeFetch({ ok: true });

		await deletePluginTools('a/b');

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/mcp/plugin-tools/a%2Fb');
	});

	it('throws the daemon own words when it refuses the set', async () => {
		fakeFetch({ ok: false, status: 400, body: { error: "tool name 'a__b' may not contain '__'." } });

		await expect(putPluginTools('hello-world', [])).rejects.toThrow(
			"tool name 'a__b' may not contain '__'."
		);
	});

	it('falls back to the status when the refusal is not JSON', async () => {
		fakeFetch({ ok: false, status: 500 });

		await expect(putPluginTools('hello-world', [])).rejects.toThrow('the daemon answered 500');
	});
});
