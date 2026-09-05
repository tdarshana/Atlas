// @vitest-environment jsdom
// The global MCP servers view's two write paths. `Check all` starts one of the user's own
// commands per row, so it names the count and waits to be told to go, and it holds the row
// actions while it walks the list. An add says which file the server landed in, because
// the default target is a `.mcp.json` the daemon creates and nobody can guess.

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import type { McpCheckResult, McpServerEntry, McpServerList, NewMcpServer } from '$lib/types';

beforeAll(() => {
	HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
		this.open = false;
	};
});

function entry(name: string, extra: Partial<McpServerEntry> = {}): McpServerEntry {
	return {
		id: `claude:user:${name}`,
		name,
		source: 'claude',
		scope: 'user',
		transport: { kind: 'stdio', command: 'npx', args: [], env_keys: [] },
		file: '/home/u/.claude.json',
		plugin: null,
		enabled: true,
		can_toggle: true,
		can_remove: true,
		is_atlas: false,
		project_id: null,
		...extra
	};
}

const list: McpServerList = {
	servers: [
		entry('atlas', { id: 'atlas', source: 'atlas', is_atlas: true, can_toggle: false, file: null }),
		entry('fs'),
		entry('gh')
	],
	warnings: []
};

const result: McpCheckResult = {
	ok: true,
	server_name: 'fs',
	server_version: '1.0.0',
	protocol_version: '2025-06-18',
	tools: [],
	error: null,
	elapsed_ms: 12
};

const checked: string[] = [];
/** Held open by a test that wants to look at the page mid-run. */
let holdCheck: (() => void) | null = null;

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listMcpServers: async () => list,
		checkMcpServer: async (id: string) => {
			checked.push(id);
			if (holdCheck) await new Promise<void>((resolve) => (holdCheck = resolve));
			return result;
		},
		addMcpServer: async (input: NewMcpServer) => entry(input.name, { file: '/repo/.mcp.json' })
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import McpPage from './+page.svelte';
import { servers } from '$lib/stores/mcp-servers.svelte';
import { clear, toasts } from '$lib/platform/toasts.svelte';

beforeEach(() => {
	checked.length = 0;
	holdCheck = null;
	clear();
	servers.items = [];
	servers.checks = {};
	servers.openId = null;
	servers.sourceFilter = null;
	servers.busyId = null;
});

afterEach(() => {
	cleanup();
	servers.items = [];
});

describe('the MCP servers view', () => {
	it('names the count and starts nothing until Check all is confirmed', async () => {
		render(McpPage);
		await waitFor(() => screen.getByTestId('mcp-server-source-claude:user:fs'));

		await fireEvent.click(screen.getByTestId('mcp-check-all'));
		expect(checked).toEqual([]);
		expect(screen.getByTestId('mcp-check-all-confirm').textContent).toContain('Start 3 servers');

		await fireEvent.click(screen.getByTestId('mcp-check-all-start'));
		await waitFor(() => expect(checked).toEqual(['atlas', 'claude:user:fs', 'claude:user:gh']));
	});

	it('holds the row actions while it walks the list', async () => {
		holdCheck = () => {};
		render(McpPage);
		await waitFor(() => screen.getByTestId('mcp-server-source-claude:user:fs'));

		await fireEvent.click(screen.getByTestId('mcp-check-all'));
		await fireEvent.click(screen.getByTestId('mcp-check-all-start'));

		const toggle = screen.getByTestId('mcp-server-toggle-claude:user:fs') as HTMLInputElement;
		await waitFor(() => expect(toggle.disabled).toBe(true));
		expect((screen.getByTestId('mcp-check-all') as HTMLButtonElement).disabled).toBe(true);
	});

	it('says which file a new server landed in', async () => {
		render(McpPage);
		await waitFor(() => screen.getByTestId('mcp-server-source-claude:user:fs'));

		await fireEvent.click(screen.getByTestId('mcp-add-server'));
		await fireEvent.input(screen.getByTestId('add-server-name'), { target: { value: 'files' } });
		await fireEvent.input(screen.getByTestId('add-server-command'), { target: { value: 'npx' } });
		await fireEvent.click(screen.getByTestId('add-server-submit'));

		await waitFor(() => expect(toasts.length).toBe(1));
		expect(toasts[0].text).toBe('MCP server added to /repo/.mcp.json');
	});
});
