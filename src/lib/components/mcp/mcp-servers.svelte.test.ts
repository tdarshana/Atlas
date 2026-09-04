// @vitest-environment jsdom
// What the MCP servers table, the detail panel and the add dialog actually do: the table
// names each agent and only offers a checkbox where the agent has a switch, the detail
// shows the transport and the last check without ever showing a secret, and the dialog
// refuses a bad name and sends the entry the daemon expects.

import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import type {
	McpCheckResult,
	McpServerEntry,
	McpServerSource,
	NewMcpServer
} from '$lib/types';

// jsdom carries `<dialog>` but not its top layer, so `showModal` is missing. The dialog
// only needs to be in the document for these assertions, so stand the two calls in.
beforeAll(() => {
	HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
		this.open = false;
	};
});

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import AddServerDialog from './AddServerDialog.svelte';
import ServerDetail from './ServerDetail.svelte';
import ServerTable from './ServerTable.svelte';

afterEach(cleanup);

function entry(
	name: string,
	source: McpServerSource,
	extra: Partial<McpServerEntry> = {}
): McpServerEntry {
	return {
		id: `${source}:user:${name}`,
		name,
		source,
		scope: 'user',
		transport: { kind: 'stdio', command: 'npx', args: ['-y', 'server'], env_keys: [] },
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

const tableProps = {
	id: 'mcp-servers',
	checks: {} as Record<string, McpCheckResult>,
	onopen: () => {},
	ontoggle: () => {},
	oncheck: () => {},
	onremove: () => {}
};

describe('ServerTable', () => {
	it('names the agent each row came from, plugin included', () => {
		render(ServerTable, {
			props: {
				...tableProps,
				rows: [
					entry('fs', 'claude'),
					entry('codex-one', 'codex'),
					entry('docs', 'plugin', { scope: 'plugin', plugin: 'anthropics/docs' })
				]
			}
		});

		expect(screen.getByTestId('mcp-server-source-claude:user:fs').textContent).toContain(
			'Claude Code'
		);
		expect(screen.getByTestId('mcp-server-source-codex:user:codex-one').textContent).toContain(
			'Codex'
		);
		const plugin = screen.getByTestId('mcp-server-source-plugin:user:docs');
		expect(plugin.textContent).toContain('Plugin');
		expect(plugin.getAttribute('title')).toBe('anthropics/docs');
	});

	it('gives a checkbox only where the agent has a switch, and a reason where it has none', () => {
		render(ServerTable, {
			props: {
				...tableProps,
				rows: [
					entry('fs', 'claude'),
					entry('codex-one', 'codex', { can_toggle: false }),
					entry('atlas', 'atlas', { id: 'atlas', can_toggle: false, is_atlas: true })
				]
			}
		});

		expect(screen.getByTestId('mcp-server-toggle-claude:user:fs')).toBeTruthy();
		expect(screen.queryByTestId('mcp-server-toggle-codex:user:codex-one')).toBeNull();
		expect(
			screen.getByTestId('mcp-server-fixed-codex:user:codex-one').getAttribute('title')
		).toContain('remove the server instead');
		expect(screen.getByTestId('mcp-server-fixed-atlas').getAttribute('title')).toContain('Atlas');
	});

	it('reports the new state when Enabled is unticked, without opening the row', async () => {
		const ontoggle = vi.fn();
		const onopen = vi.fn();
		render(ServerTable, { props: { ...tableProps, rows: [entry('fs', 'claude')], ontoggle, onopen } });

		const box = screen.getByTestId('mcp-server-toggle-claude:user:fs') as HTMLInputElement;
		expect(box.checked).toBe(true);
		await fireEvent.click(box);
		expect(ontoggle.mock.calls[0][0].id).toBe('claude:user:fs');
		expect(ontoggle.mock.calls[0][1]).toBe(false);
		expect(onopen).not.toHaveBeenCalled();
	});

	it('leaves Status empty until the row has been checked, then counts its tools', () => {
		const checks: Record<string, McpCheckResult> = {
			'claude:user:fs': {
				ok: true,
				server_name: 'filesystem',
				server_version: '1.0.0',
				protocol_version: '2025-06-18',
				tools: [{ name: 'read_file', description: 'Reads a file' }],
				error: null,
				elapsed_ms: 412
			}
		};
		const { unmount } = render(ServerTable, {
			props: { ...tableProps, rows: [entry('fs', 'claude')] }
		});
		expect(screen.getByTestId('mcp-server-status-claude:user:fs').textContent?.trim()).toBe('');
		unmount();

		render(ServerTable, { props: { ...tableProps, rows: [entry('fs', 'claude')], checks } });
		const cell = screen.getByTestId('mcp-server-status-claude:user:fs');
		expect(cell.textContent).toContain('1 tool');
		expect(cell.getAttribute('title')).toBe('Checked in 412 ms');
	});

	it('offers Remove in the row menu only where the entry can be removed', async () => {
		const onremove = vi.fn();
		render(ServerTable, {
			props: {
				...tableProps,
				rows: [entry('atlas', 'atlas', { id: 'atlas', can_remove: false, is_atlas: true })],
				onremove
			}
		});

		await fireEvent.click(screen.getByTestId('mcp-server-menu-atlas'));
		expect(screen.getByTestId('mcp-server-check-atlas')).toBeTruthy();
		expect(screen.queryByTestId('mcp-server-remove-atlas')).toBeNull();
	});
});

describe('ServerDetail', () => {
	const base = {
		width: 380,
		onclose: () => {},
		onresize: () => {},
		oncheck: () => {},
		ontoggle: () => {},
		onremove: () => {}
	};

	it('shows the transport and the secret key names, never their values', () => {
		render(ServerDetail, {
			props: {
				...base,
				check: null,
				server: entry('gh', 'claude', {
					transport: {
						kind: 'http',
						url: 'https://api.github.com/mcp',
						header_keys: ['Authorization']
					}
				})
			}
		});

		const panel = screen.getByTestId('mcp-server-detail');
		expect(panel.textContent).toContain('https://api.github.com/mcp');
		expect(screen.getByTestId('mcp-server-detail-secrets').textContent).toContain('Authorization');
		expect(panel.textContent).toContain('••••••••');
	});

	it('lists the tools the last check found', () => {
		render(ServerDetail, {
			props: {
				...base,
				server: entry('fs', 'claude'),
				check: {
					ok: true,
					server_name: 'filesystem',
					server_version: '1.0.0',
					protocol_version: '2025-06-18',
					tools: [{ name: 'read_file', description: 'Reads a file' }],
					error: null,
					elapsed_ms: 412
				}
			}
		});

		const result = screen.getByTestId('mcp-server-detail-check-result');
		expect(result.textContent).toContain('filesystem 1.0.0');
		expect(result.textContent).toContain('2025-06-18');
		expect(result.textContent).toContain('read_file');
	});

	it('says why a failed check failed', () => {
		render(ServerDetail, {
			props: {
				...base,
				server: entry('fs', 'claude'),
				check: {
					ok: false,
					server_name: null,
					server_version: null,
					protocol_version: null,
					tools: [],
					error: 'command not found: npx',
					elapsed_ms: 12
				}
			}
		});
		expect(screen.getByTestId('mcp-server-detail-check-result').textContent).toContain(
			'command not found: npx'
		);
	});

	it('offers Enable or Disable only where the agent has a switch', () => {
		const { unmount } = render(ServerDetail, {
			props: { ...base, server: entry('fs', 'claude'), check: null }
		});
		expect(screen.getByTestId('mcp-server-detail-toggle').textContent).toContain('Disable');
		unmount();

		render(ServerDetail, {
			props: {
				...base,
				server: entry('atlas', 'atlas', {
					id: 'atlas',
					can_toggle: false,
					can_remove: false,
					is_atlas: true,
					file: null
				}),
				check: null
			}
		});
		expect(screen.queryByTestId('mcp-server-detail-toggle')).toBeNull();
		expect(screen.getByTestId('mcp-server-detail-no-toggle').textContent).toContain('Atlas');
		expect(screen.queryByTestId('mcp-server-detail-remove')).toBeNull();
	});
});

describe('AddServerDialog', () => {
	const base = { open: true, onclose: () => {} };

	it('refuses a name the daemon would refuse', async () => {
		const onadd = vi.fn(async (_input: NewMcpServer) => {});
		render(AddServerDialog, { props: { ...base, projectId: null, onadd } });

		await fireEvent.input(screen.getByTestId('add-server-name'), {
			target: { value: 'no spaces here' }
		});
		await fireEvent.input(screen.getByTestId('add-server-command'), { target: { value: 'npx' } });

		expect((screen.getByTestId('add-server-submit') as HTMLButtonElement).disabled).toBe(true);
		expect(onadd).not.toHaveBeenCalled();
	});

	it('sends the stdio entry with one argument per line and the env values once', async () => {
		const onadd = vi.fn(async (_input: NewMcpServer) => {});
		render(AddServerDialog, { props: { ...base, projectId: null, onadd } });

		await fireEvent.input(screen.getByTestId('add-server-name'), { target: { value: 'files' } });
		await fireEvent.input(screen.getByTestId('add-server-command'), { target: { value: 'npx' } });
		await fireEvent.input(screen.getByTestId('add-server-args'), {
			target: { value: '-y\n@modelcontextprotocol/server-filesystem\n' }
		});
		await fireEvent.input(screen.getByTestId('add-server-key-0'), { target: { value: 'TOKEN' } });
		await fireEvent.input(screen.getByTestId('add-server-value-0'), { target: { value: 's3cret' } });
		await fireEvent.click(screen.getByTestId('add-server-submit'));

		expect(onadd).toHaveBeenCalledWith({
			source: 'claude',
			scope: 'user',
			project_id: null,
			name: 'files',
			transport: {
				kind: 'stdio',
				command: 'npx',
				args: ['-y', '@modelcontextprotocol/server-filesystem'],
				env: { TOKEN: 's3cret' }
			}
		});
	});

	it('offers this project’s own files first, and carries the project id', async () => {
		const onadd = vi.fn(async (_input: NewMcpServer) => {});
		render(AddServerDialog, { props: { ...base, projectId: 'p-1', onadd } });

		const select = screen.getByTestId('add-server-target') as HTMLSelectElement;
		expect(select.value).toBe('claude:project');

		await fireEvent.input(screen.getByTestId('add-server-name'), { target: { value: 'files' } });
		await fireEvent.input(screen.getByTestId('add-server-command'), { target: { value: 'npx' } });
		await fireEvent.click(screen.getByTestId('add-server-submit'));

		expect(onadd.mock.calls[0][0]).toMatchObject({
			source: 'claude',
			scope: 'project',
			project_id: 'p-1'
		});
	});

	it('sends an HTTP entry with its headers', async () => {
		const onadd = vi.fn(async (_input: NewMcpServer) => {});
		render(AddServerDialog, { props: { ...base, projectId: null, onadd } });

		await fireEvent.click(screen.getByTestId('add-server-kind-http'));
		await fireEvent.input(screen.getByTestId('add-server-name'), { target: { value: 'remote' } });
		await fireEvent.input(screen.getByTestId('add-server-url'), {
			target: { value: 'https://mcp.example.com/sse' }
		});
		await fireEvent.input(screen.getByTestId('add-server-key-0'), {
			target: { value: 'Authorization' }
		});
		await fireEvent.input(screen.getByTestId('add-server-value-0'), {
			target: { value: 'Bearer x' }
		});
		await fireEvent.click(screen.getByTestId('add-server-submit'));

		expect(onadd.mock.calls[0][0].transport).toEqual({
			kind: 'http',
			url: 'https://mcp.example.com/sse',
			headers: { Authorization: 'Bearer x' }
		});
	});
});
