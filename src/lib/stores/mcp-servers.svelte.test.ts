// @vitest-environment jsdom
// The MCP server store's writes: `Enabled here` carries the project the list was loaded
// for, a check is kept under its server's id, and both are followed by a reload so the
// row shows the daemon's answer rather than an optimistic guess.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { McpCheckResult, McpServerEntry, McpServerList } from '$lib/types';

const entry: McpServerEntry = {
	id: 'claude:project:fs',
	name: 'fs',
	source: 'claude',
	scope: 'project',
	transport: { kind: 'stdio', command: 'npx', args: [], env_keys: [] },
	file: '/repo/.mcp.json',
	plugin: null,
	enabled: true,
	can_toggle: true,
	can_remove: true,
	is_atlas: false,
	project_id: 'p-1'
};

const calls: { name: string; args: unknown[] }[] = [];
const checkResult: McpCheckResult = {
	ok: true,
	server_name: 'fs',
	server_version: '1.0.0',
	protocol_version: '2025-06-18',
	tools: [{ name: 'read_file', description: 'Reads a file' }],
	error: null,
	elapsed_ms: 40
};

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listMcpServers: async (projectId: string | null): Promise<McpServerList> => {
			calls.push({ name: 'list', args: [projectId] });
			return { servers: [entry], warnings: ['~/.cursor/mcp.json is not valid JSON'] };
		},
		setMcpServerEnabled: async (id: string, enabled: boolean, projectId: string | null) => {
			calls.push({ name: 'setEnabled', args: [id, enabled, projectId] });
		},
		checkMcpServer: async (id: string, projectId: string | null) => {
			calls.push({ name: 'check', args: [id, projectId] });
			return checkResult;
		},
		removeMcpServer: async (id: string, projectId: string | null) => {
			calls.push({ name: 'remove', args: [id, projectId] });
		}
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { check, loadServers, remove, servers, setEnabled } from './mcp-servers.svelte';

beforeEach(() => {
	calls.length = 0;
	servers.items = [];
	servers.checks = {};
	servers.openId = null;
	servers.projectId = null;
});

describe('the MCP server store', () => {
	it('loads a project’s servers and keeps its warnings', async () => {
		await loadServers('p-1');

		expect(calls[0]).toEqual({ name: 'list', args: ['p-1'] });
		expect(servers.items).toHaveLength(1);
		expect(servers.warnings[0]).toContain('.cursor/mcp.json');
		expect(servers.error).toBeNull();
	});

	it('sends Enabled here with the project the list was loaded for, then reloads', async () => {
		await loadServers('p-1');
		calls.length = 0;

		await setEnabled('claude:project:fs', false);

		expect(calls[0]).toEqual({ name: 'setEnabled', args: ['claude:project:fs', false, 'p-1'] });
		expect(calls[1]).toEqual({ name: 'list', args: ['p-1'] });
		expect(servers.busyId).toBeNull();
	});

	it('keeps a check under its own server id', async () => {
		await loadServers(null);
		await check('claude:project:fs');

		expect(servers.checks['claude:project:fs'].tools).toHaveLength(1);
		expect(servers.busyId).toBeNull();
	});

	it('closes the detail panel when the open server is removed', async () => {
		await loadServers('p-1');
		servers.openId = 'claude:project:fs';

		await remove('claude:project:fs');

		expect(calls.some((c) => c.name === 'remove')).toBe(true);
		expect(servers.openId).toBeNull();
	});
});
