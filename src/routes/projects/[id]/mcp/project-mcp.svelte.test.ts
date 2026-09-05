// @vitest-environment jsdom
// The project MCP tab groups this project's servers by where they came from: the
// project's own files first, then the plugins, then Atlas. Its `Enabled here`
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

const full: McpServerList = {
	servers: [
		entry('atlas', 'atlas', { id: 'atlas', scope: 'user', is_atlas: true, can_toggle: false }),
		entry('docs', 'plugin', { scope: 'plugin', plugin: 'anthropics/docs', can_toggle: true }),
		entry('off', 'plugin', {
			scope: 'plugin',
			plugin: 'anthropics/off',
			enabled: false,
			can_toggle: false
		}),
		entry('fs', 'claude')
	],
	warnings: []
};

/** What the mocked daemon answers; a test swaps it to stand in for a different repo. */
let list: McpServerList = full;

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
import { NO_PROJECT_SERVERS } from '$lib/mcp-servers';
import { servers } from '$lib/stores/mcp-servers.svelte';

beforeEach(() => {
	calls.length = 0;
	list = full;
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

	it('keeps the Project group when the repo has no project-level server, and says where Add server… writes', async () => {
		list = {
			servers: full.servers.filter((s) => s.source !== 'claude'),
			warnings: []
		};
		render(ProjectMcpPage);

		// Wait for the loaded list, not for the empty group: an empty `Project` group is
		// also what the very first render draws, before the fetch lands.
		await waitFor(() => screen.getByTestId('mcp-server-source-atlas'));
		const headings = screen.getAllByTestId('project-mcp-group').map((h) => h.textContent);
		expect(headings).toEqual(['Project', 'Plugins', 'Atlas']);
		expect(screen.getByTestId('project-mcp-servers-project-empty').textContent).toBe(
			NO_PROJECT_SERVERS
		);
	});

	it('offers no Enabled checkbox for the Atlas row or a plugin row without a switch', async () => {
		render(ProjectMcpPage);

		await waitFor(() => screen.getByTestId('mcp-server-fixed-atlas'));
		expect(screen.queryByTestId('mcp-server-toggle-atlas')).toBeNull();
		expect(screen.queryByTestId('mcp-server-toggle-plugin:project:off')).toBeNull();
	});

	it('switches a plugin server off for this project through its own row', async () => {
		render(ProjectMcpPage);

		await waitFor(() => screen.getByTestId('mcp-server-toggle-plugin:project:docs'));
		await fireEvent.click(screen.getByTestId('mcp-server-toggle-plugin:project:docs'));
		await waitFor(() => expect(calls.some((c) => c.name === 'setEnabled')).toBe(true));
		expect(calls.find((c) => c.name === 'setEnabled')?.args).toEqual([
			'plugin:project:docs',
			false,
			'p-1'
		]);
	});
});
