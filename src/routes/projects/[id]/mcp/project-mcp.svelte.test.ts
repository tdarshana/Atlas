// @vitest-environment jsdom
// The project MCP tab groups this project's servers by where they came from — the
// project's own files first, then the plugins, then Atlas — and its `Enabled here`
// checkbox writes through with the project the list was loaded for.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import type { McpServerEntry, McpServerList, Project } from '$lib/types';

function entry(
	name: string,
	source: McpServerEntry['source'],
	extra: Partial<McpServerEntry> = {}
): McpServerEntry {
	return {
		id: `${source}:project:${name}`,
		name,
		source,
		scope: 'project',
		transport: { kind: 'stdio', command: 'npx', args: [], env_keys: [] },
		file: '/repo/.mcp.json',
		plugin: null,
		enabled: true,
		can_toggle: true,
		can_remove: true,
		is_atlas: false,
		project_id: 'p-1',
		...extra
	};
}

const list: McpServerList = {
	servers: [
		entry('atlas', 'atlas', { id: 'atlas', scope: 'user', is_atlas: true, can_toggle: false }),
		entry('docs', 'plugin', { scope: 'plugin', plugin: 'anthropics/docs', can_toggle: false }),
		entry('fs', 'claude')
	],
	warnings: []
};

const calls: { name: string; args: unknown[] }[] = [];

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listMcpServers: async (projectId: string | null) => {
			calls.push({ name: 'list', args: [projectId] });
			return list;
		},
		setMcpServerEnabled: async (id: string, enabled: boolean, projectId: string | null) => {
			calls.push({ name: 'setEnabled', args: [id, enabled, projectId] });
		}
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

// `project.current` is a getter over the projects store, and the tab only ever reads it,
// so the store stands in as a plain object here rather than being driven through a load.
vi.mock('$lib/stores/project.svelte', () => ({
	project: { current: { id: 'p-1', name: 'atlas', root_path: '/repo' } as unknown as Project },
	setHeaderActions: () => {}
}));

import ProjectMcpPage from './+page.svelte';
import { servers } from '$lib/stores/mcp-servers.svelte';

beforeEach(() => {
	calls.length = 0;
	servers.items = [];
	servers.openId = null;
	servers.sourceFilter = null;
});

afterEach(() => {
	cleanup();
	servers.items = [];
});

describe('the project MCP tab', () => {
	it('lists this project’s servers under Project, Plugins and Atlas', async () => {
		render(ProjectMcpPage);

		await waitFor(() => screen.getByTestId('mcp-server-source-claude:project:fs'));
		const headings = screen.getAllByTestId('project-mcp-group').map((h) => h.textContent);
		expect(headings).toEqual(['Project', 'Plugins', 'Atlas']);
		expect(calls[0]).toEqual({ name: 'list', args: ['p-1'] });
	});

	it('labels the toggle column Enabled here and writes through with the project id', async () => {
		render(ProjectMcpPage);

		await waitFor(() => screen.getByTestId('mcp-server-toggle-claude:project:fs'));
		expect(screen.getAllByText('Enabled here').length).toBeGreaterThan(0);

		await fireEvent.click(screen.getByTestId('mcp-server-toggle-claude:project:fs'));
		await waitFor(() => expect(calls.some((c) => c.name === 'setEnabled')).toBe(true));
		expect(calls.find((c) => c.name === 'setEnabled')?.args).toEqual([
			'claude:project:fs',
			false,
			'p-1'
		]);
	});

	it('offers no Enabled checkbox for the Atlas or plugin rows', async () => {
		render(ProjectMcpPage);

		await waitFor(() => screen.getByTestId('mcp-server-fixed-atlas'));
		expect(screen.queryByTestId('mcp-server-toggle-atlas')).toBeNull();
		expect(screen.queryByTestId('mcp-server-toggle-plugin:project:docs')).toBeNull();
	});
});
