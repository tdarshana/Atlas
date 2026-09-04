// The MCP server list, shared by the `/mcp` route, the project MCP tab and the MCP side
// panel, so the counts the panel shows come from the list the table drew. The list is
// per scope: loading a project's servers replaces the global list, because the two views
// are never on screen at the same time.

import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import { clampDetail, DETAIL_DEFAULT, DETAIL_KEY } from '$lib/mcp-servers';
import { persistSet } from '$lib/shell/persist';
import type { McpCheckResult, McpServerEntry, NewMcpServer, Uuid } from '$lib/types';

export const servers = $state({
	items: [] as McpServerEntry[],
	warnings: [] as string[],
	/** The last check per server id, kept for as long as the view is open. */
	checks: {} as Record<string, McpCheckResult>,
	loading: false,
	error: null as string | null,
	/** The project the list was loaded for, or null for the global list. */
	projectId: null as Uuid | null,
	/** The row open in the detail panel. */
	openId: null as string | null,
	/**
	 * The section a side panel jump row asked the open detail to scroll to (`tools`,
	 * `resources`, `prompts`, `clients`). It travels through the store rather than the
	 * URL hash on purpose: a hash makes the browser scroll every scrollable ancestor,
	 * which dragged the table's own region out of its frame.
	 */
	section: null as string | null,
	/** The id being checked or written right now, so its controls hold still. */
	busyId: null as string | null,
	/** The Agent filter the side panel sets; null means every agent. */
	sourceFilter: null as string | null,
	detailWidth: loadDetailWidth()
});

export async function loadServers(projectId: Uuid | null = null): Promise<void> {
	servers.loading = true;
	servers.projectId = projectId;
	try {
		const list = await api().listMcpServers(projectId);
		servers.items = list.servers;
		servers.warnings = list.warnings;
		// A check belongs to the listing it was run against. Ids are stable by
		// construction, so a server removed and added back under the same name would
		// otherwise show the old run's tool count as if it were its own.
		servers.checks = {};
		servers.error = null;
	} catch (e) {
		servers.items = [];
		servers.warnings = [];
		servers.checks = {};
		servers.error = errorMessage(e);
	} finally {
		servers.loading = false;
	}
}

/** Starts one server and keeps its answer under its id. Throws on a transport failure;
 * a server that started and refused is a result with `ok: false`. */
export async function check(id: string): Promise<McpCheckResult> {
	servers.busyId = id;
	try {
		const result = await api().checkMcpServer(id, servers.projectId);
		servers.checks = { ...servers.checks, [id]: result };
		return result;
	} finally {
		servers.busyId = null;
	}
}

/** Checks every listed server one at a time: each one starts a real process, so running
 * them together would spawn the whole list at once. */
export async function checkAll(): Promise<void> {
	for (const server of servers.items) {
		try {
			await check(server.id);
		} catch (e) {
			servers.checks = {
				...servers.checks,
				[server.id]: {
					ok: false,
					server_name: null,
					server_version: null,
					protocol_version: null,
					tools: [],
					error: errorMessage(e),
					elapsed_ms: 0
				}
			};
		}
	}
}

/** Flips the agent's own switch, then reloads so the row shows the daemon's answer
 * rather than an optimistic guess. */
export async function setEnabled(id: string, enabled: boolean): Promise<void> {
	servers.busyId = id;
	try {
		await api().setMcpServerEnabled(id, enabled, servers.projectId);
		await loadServers(servers.projectId);
	} finally {
		servers.busyId = null;
	}
}

/** Deletes one server from the file it came from. `busyId` is held for the round trip so
 * a second click on the confirm cannot fire a second DELETE, whose 404 the dialog would
 * show as an error. */
export async function remove(id: string): Promise<void> {
	if (servers.busyId === id) return;
	servers.busyId = id;
	try {
		await api().removeMcpServer(id, servers.projectId);
	} finally {
		servers.busyId = null;
	}
	if (servers.openId === id) servers.openId = null;
	await loadServers(servers.projectId);
}

export async function add(input: NewMcpServer): Promise<McpServerEntry> {
	const created = await api().addMcpServer(input);
	await loadServers(servers.projectId);
	return created;
}

/** Opens one row in the detail panel, optionally asking it for a section. */
export function openServer(id: string | null, section: string | null = null): void {
	servers.openId = id;
	servers.section = section;
}

/** Forgets the section once the detail has scrolled to it, so reopening the same server
 * lands at the top rather than wherever a side panel row sent it last time. */
export function clearSection(): void {
	servers.section = null;
}

export function setDetailWidth(width: number): void {
	servers.detailWidth = clampDetail(width);
	saveDetailWidth(servers.detailWidth);
}

function loadDetailWidth(): number {
	try {
		if (typeof localStorage === 'undefined') return DETAIL_DEFAULT;
		const raw = localStorage.getItem(DETAIL_KEY);
		if (raw === null) return DETAIL_DEFAULT;
		return clampDetail(Number(raw));
	} catch {
		return DETAIL_DEFAULT;
	}
}

function saveDetailWidth(width: number): void {
	try {
		if (typeof localStorage !== 'undefined') localStorage.setItem(DETAIL_KEY, String(width));
	} catch {
		/* a webview with storage denied still resizes, it just forgets */
	}
	void persistSet(DETAIL_KEY, width);
}
