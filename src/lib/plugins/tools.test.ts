// The daemon's plugin tool channel, driven against a fake socket. The channel never
// touches the network here: `socketFactory` hands back an object a test opens, feeds and
// closes by hand, which is the whole reason the socket is a parameter.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	channelUrl,
	createToolChannel,
	toolDecls,
	toolPlugins,
	toolRegistry,
	type PluginTools,
	type ToolChannel,
	type ToolSocket
} from './tools';
import type { Manifest, Permission, PluginInfo, ToolContribution } from './types';

vi.mock('$lib/daemon.svelte', () => ({
	baseUrl: () => 'http://127.0.0.1:7433'
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
	return { id, manifest, enabled: true, compatible: true, reason: null, dir: `/p/${id}`, ...overrides };
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
	const register = vi.fn(async () => {});
	const unregister = vi.fn(async () => {});
	const onCall = vi.fn(async () => ({ count: 3 }));
	const onError = vi.fn();
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
		onError
	});
	return { channel, sockets, register, unregister, onCall, onError, latest: () => sockets[sockets.length - 1] };
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
			"plugin no-permission declares MCP tools without the 'mcp.tools' permission"
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
