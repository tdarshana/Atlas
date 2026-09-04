// The bridge's rules: a method a plugin does not hold the permission for is refused
// before anything is proxied, ids come back matched even when two requests overlap,
// another window's messages are not this frame's, and `settings.get` is narrower than the
// permission that gates it.

import { describe, expect, it, vi } from 'vitest';
import type { Task } from '$lib/types';
import { createBridge, MAX_FRAME_HEIGHT, MIN_FRAME_HEIGHT, type PostTarget } from './bridge';
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

function harness(permissions: Permission[]) {
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
		dir: '/plugins/hello-world'
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
});
