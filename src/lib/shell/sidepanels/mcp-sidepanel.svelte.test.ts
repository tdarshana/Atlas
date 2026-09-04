// @vitest-environment jsdom
// The MCP side panel lists one row per agent that actually contributed a server, and
// clicking a row filters the table to that agent, then clears the filter when clicked
// again.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import type { McpServerEntry, McpServerList, McpStatusReport } from '$lib/types';

function entry(name: string, source: McpServerEntry['source']): McpServerEntry {
	return {
		id: `${source}:user:${name}`,
		name,
		source,
		scope: 'user',
		transport: { kind: 'stdio', command: 'npx', args: [], env_keys: [] },
		file: '/home/u/.claude.json',
		plugin: null,
		enabled: true,
		can_toggle: true,
		can_remove: true,
		is_atlas: source === 'atlas',
		project_id: null
	};
}

const list: McpServerList = {
	servers: [entry('a', 'claude'), entry('b', 'claude'), entry('c', 'cursor'), entry('atlas', 'atlas')],
	warnings: []
};

const report: McpStatusReport = {
	transports: {
		stdio: { command: 'atlas mcp' },
		http: { url: 'http://127.0.0.1:7433/mcp', protocol_version: '2025-06-18' }
	},
	counts: { tools: 1, resources: 0, prompts: 0, clients: 0 },
	tools: [
		{ name: 'memory_remember', description: '', args: '', scope: 'write', enabled: false }
	],
	resources: [],
	prompts: [],
	clients: []
};

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ listMcpServers: async () => list, mcpStatus: async () => report }),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import McpPanel from './Mcp.svelte';
import { mcp } from '$lib/stores/mcp.svelte';
import { servers } from '$lib/stores/mcp-servers.svelte';

beforeEach(() => {
	servers.items = list.servers;
	servers.loading = false;
	servers.sourceFilter = null;
	mcp.report = report;
	mcp.loading = false;
});

afterEach(() => {
	cleanup();
	servers.items = [];
	mcp.report = null;
});

describe('the MCP side panel', () => {
	it('counts the servers each agent contributed', () => {
		render(McpPanel);

		expect(screen.getByText('Claude Code').closest('.row')?.textContent).toContain('2');
		expect(screen.getByText('Cursor').closest('.row')?.textContent).toContain('1');
		expect(screen.queryByText('Codex')).toBeNull();
	});

	it('filters the table to one agent, and clears the filter on a second click', async () => {
		render(McpPanel);

		await fireEvent.click(screen.getByText('Cursor'));
		expect(servers.sourceFilter).toBe('cursor');

		await fireEvent.click(screen.getByText('Cursor'));
		expect(servers.sourceFilter).toBeNull();
	});

	it('still shows the Atlas jump rows with their own counts', () => {
		render(McpPanel);

		expect(screen.getByText('Tools').closest('.row')?.textContent).toContain('1 · 1 off');
		expect(screen.getByText('Clients')).toBeTruthy();
	});
});
