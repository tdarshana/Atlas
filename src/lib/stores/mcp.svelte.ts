// Global MCP state: the `GET /api/v1/mcp/status` report the `/mcp` route and its side
// panel both read, so a toggle on the route is visible in the panel's counts without a
// second fetch. The Settings screen's two-line summary reads the same store.

import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import { nextDisabledTools } from '$lib/mcp';
import type { McpStatusReport, McpToolRow } from '$lib/types';

export const mcp = $state({
	report: null as McpStatusReport | null,
	loading: false,
	error: null as string | null
});

export async function loadMcp(): Promise<void> {
	mcp.loading = true;
	try {
		mcp.report = await api().mcpStatus();
		mcp.error = null;
	} catch (e) {
		mcp.error = errorMessage(e);
	} finally {
		mcp.loading = false;
	}
}

/** Flips one tool's global checkbox: writes `mcp.disabled_tools` with that name added
 * or removed, leaving every other disabled tool untouched, then reloads the report. */
export async function toggleTool(row: McpToolRow): Promise<void> {
	if (!mcp.report) return;
	const currentlyDisabled = mcp.report.tools.filter((t) => !t.enabled).map((t) => t.name);
	const next = nextDisabledTools(currentlyDisabled, row.name, !row.enabled);
	await api().setSettings({ 'mcp.disabled_tools': next });
	await loadMcp();
}
