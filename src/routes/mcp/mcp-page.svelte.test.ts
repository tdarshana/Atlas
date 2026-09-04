// @vitest-environment jsdom
// The tools table tells a plugin's tool apart from a built-in, which matters because a
// plugin tool is only answerable while the app is running.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/svelte';
import type { McpStatusReport } from '$lib/types';

const report: McpStatusReport = {
	transports: {
		stdio: { command: 'atlas mcp' },
		http: { url: 'http://127.0.0.1:7433/mcp', protocol_version: '2025-06-18' }
	},
	counts: { tools: 2, resources: 0, prompts: 0, clients: 0 },
	tools: [
		{
			name: 'memory_remember',
			description: 'Writes a memory.',
			args: 'text*',
			scope: 'write',
			enabled: true,
			source: 'builtin'
		},
		{
			name: 'plugin__hello_world__ready_count',
			description: 'Count the ready tasks on the board',
			args: 'none',
			scope: 'read',
			enabled: true,
			source: 'plugin:hello-world'
		}
	],
	resources: [],
	prompts: [],
	clients: []
};

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ mcpStatus: async () => report }),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import McpPage from './+page.svelte';
import { mcp } from '$lib/stores/mcp.svelte';

beforeEach(() => {
	mcp.report = report;
	mcp.loading = false;
	mcp.error = null;
});

afterEach(() => {
	cleanup();
	mcp.report = null;
});

describe('the MCP tools table', () => {
	it('badges a plugin tool with the plugin it came from, and leaves a built-in bare', async () => {
		const { getByTestId, queryByTestId } = render(McpPage);

		await waitFor(() => getByTestId('mcp-tool-source-plugin__hello_world__ready_count'));
		expect(getByTestId('mcp-tool-source-plugin__hello_world__ready_count').textContent).toContain(
			'Plugin hello-world'
		);
		expect(queryByTestId('mcp-tool-source-memory_remember')).toBeNull();
	});

	it('gives a plugin tool the same global enable toggle as a built-in', () => {
		const { getByTestId } = render(McpPage);

		expect(getByTestId('mcp-tool-toggle-plugin__hello_world__ready_count')).toBeTruthy();
	});
});
