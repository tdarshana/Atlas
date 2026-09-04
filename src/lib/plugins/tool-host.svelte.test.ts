// @vitest-environment jsdom
// The hidden frames the tool host keeps: one per plugin that contributes an MCP tool, and
// none for a plugin that contributes none. The channel is faked at the socket, so nothing
// here opens a connection.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/svelte';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

const sockets: { close: () => void }[] = [];
vi.stubGlobal(
	'WebSocket',
	class {
		onopen: (() => void) | null = null;
		onmessage: ((event: { data: unknown }) => void) | null = null;
		onclose: (() => void) | null = null;
		onerror: (() => void) | null = null;
		constructor() {
			sockets.push(this);
		}
		send() {}
		close() {}
	}
);

import ToolHost from './ToolHost.svelte';
import { plugins } from './host.svelte';
import type { Contributes, Manifest, Permission, PluginInfo } from './types';

function plugin(id: string, contributes: Partial<Contributes>, permissions: Permission[]): PluginInfo {
	const manifest: Manifest = {
		id,
		name: id,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		permissions,
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [], ...contributes }
	};
	return { id, manifest, enabled: true, compatible: true, reason: null, dir: `/p/${id}` };
}

const READY_COUNT = {
	name: 'ready_count',
	description: 'Count the ready tasks on the board',
	args: { type: 'object', properties: {}, additionalProperties: false },
	scope: 'read'
};

const HELLO = plugin('hello-world', { tools: [READY_COUNT] }, ['mcp.tools']);
const QUIET = plugin('quiet', { sections: [{ id: 's', title: 'S', icon: 'plug', view: 'v' }] }, [
	'ui.sections'
]);
const UNPERMITTED = plugin('sneaky', { tools: [READY_COUNT] }, ['tasks.read']);

beforeEach(() => {
	sockets.length = 0;
	plugins.items = [];
	plugins.loaded = true;
	plugins.available = true;
	vi.spyOn(console, 'warn').mockImplementation(() => {});
});

afterEach(() => {
	cleanup();
	plugins.items = [];
	plugins.loaded = false;
	vi.restoreAllMocks();
});

describe('ToolHost', () => {
	it('mounts one hidden frame per tool-contributing plugin, and none for the rest', async () => {
		plugins.items = [HELLO, QUIET];

		const { getByTestId, queryByTestId } = render(ToolHost);

		await waitFor(() => getByTestId('plugin-background-hello-world'));
		expect(queryByTestId('plugin-background-quiet')).toBeNull();
	});

	it('mounts nothing for a plugin that declares tools without the permission', () => {
		plugins.items = [UNPERMITTED];

		const { queryByTestId } = render(ToolHost);

		expect(queryByTestId('plugin-background-sneaky')).toBeNull();
	});

	it('follows the plugin set as plugins arrive and go', async () => {
		const { queryByTestId } = render(ToolHost);
		expect(queryByTestId('plugin-background-hello-world')).toBeNull();

		plugins.items = [HELLO];
		await waitFor(() => expect(queryByTestId('plugin-background-hello-world')).not.toBeNull());

		plugins.items = [{ ...HELLO, enabled: false }];
		await waitFor(() => expect(queryByTestId('plugin-background-hello-world')).toBeNull());
	});

	it('opens the daemon channel only while some plugin contributes a tool', async () => {
		render(ToolHost);
		expect(sockets).toHaveLength(0);

		plugins.items = [HELLO];
		await waitFor(() => expect(sockets).toHaveLength(1));
	});

	it('renders nothing at all outside Tauri', () => {
		plugins.available = false;
		plugins.items = [HELLO];

		const { queryByTestId } = render(ToolHost);

		expect(queryByTestId('plugin-background-hello-world')).toBeNull();
		expect(sockets).toHaveLength(0);
	});
});
