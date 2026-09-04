// Pure helpers for the global MCP view and the project MCP tab: the connect snippets,
// the disabled-tools diff a checkbox toggle sends, the icon a tool's name maps to, a
// project tool row's three-way state, and the side panel's jump counts. No Svelte
// here, so these are plain unit tests.

import type { McpStatusReport } from './types';

export const CLAUDE_SNIPPET = 'claude mcp add --scope user atlas -- atlas mcp';
export const CODEX_SNIPPET = `# ~/.codex/config.toml
[mcp_servers.atlas]
command = "atlas"
args = ["mcp"]`;

/**
 * The `mcp.disabled_tools` array to save after toggling one tool's checkbox: `name`
 * added if the tool was just turned off, removed if it was just turned on. `current`
 * is the disabled set before the toggle; everything else in it is left untouched.
 */
export function nextDisabledTools(current: string[], name: string, enabled: boolean): string[] {
	if (enabled) return current.filter((n) => n !== name);
	return current.includes(name) ? current : [...current, name];
}

/** Icon prefixes tried in order; the first match wins. `workflow_status` must be
 * checked before the bare `workflow_` prefix. */
const TOOL_ICON_PREFIXES: [string, string][] = [
	['plugin__', 'plug'],
	['memory_', 'database'],
	['project_', 'folder'],
	['task_', 'columns-3'],
	['board_', 'columns-3'],
	['practice_', 'book-open'],
	['agent_', 'bot'],
	['workflow_status', 'history'],
	['workflow_', 'git-branch'],
	['ingest_', 'file']
];

/** The DS icon name for a tool row. Falls back to a generic tool glyph for `status`
 * and anything not covered by a prefix above. */
export function toolIcon(name: string): string {
	for (const [prefix, icon] of TOOL_ICON_PREFIXES) {
		if (name.startsWith(prefix)) return icon;
	}
	return 'terminal';
}

/** The prefix the daemon puts on a plugin tool row's `source`. */
const PLUGIN_SOURCE_PREFIX = 'plugin:';

/**
 * The plugin a tool row came from, or null for a built-in. The daemon sends
 * `plugin:<id>`; an older daemon sends no `source` at all, which reads as built-in.
 */
export function toolPluginId(source: string | undefined): string | null {
	if (!source || !source.startsWith(PLUGIN_SOURCE_PREFIX)) return null;
	const id = source.slice(PLUGIN_SOURCE_PREFIX.length);
	return id === '' ? null : id;
}

/**
 * A project MCP tab row's actual state: `enabled_globally` and `enabled_here` are the
 * same two gates `call_tool` applies, in the same order, so this mirrors that check
 * rather than re-deriving it. A tool disabled globally shows as disabled regardless of
 * the project's own override, since the project checkbox cannot re-enable it.
 */
export type ProjectMcpToolState = 'enabled' | 'disabled_here' | 'disabled_globally';

export function projectToolState(row: { enabled_globally: boolean; enabled_here: boolean }): ProjectMcpToolState {
	if (!row.enabled_globally) return 'disabled_globally';
	return row.enabled_here ? 'enabled' : 'disabled_here';
}

/** The connect snippet a project's own MCP tab shows: the project-scoped install
 * command plus the `project_root` value the tools accept, so copying it hands an
 * agent both pieces it needs. */
export function projectConnectSnippet(projectRoot: string): string {
	return `claude mcp add --scope project atlas -- atlas mcp\n# project_root: ${projectRoot}`;
}

/** The global MCP side panel's four jump rows: total tools plus how many are
 * currently disabled, and the resource/prompt/client counts. Null before the first
 * load, when every count reads as 0. */
export function mcpSidepanelCounts(report: McpStatusReport | null): {
	tools: number;
	toolsDisabled: number;
	resources: number;
	prompts: number;
	clients: number;
} {
	const tools = report?.tools ?? [];
	return {
		tools: tools.length,
		toolsDisabled: tools.filter((t) => !t.enabled).length,
		resources: report?.resources.length ?? 0,
		prompts: report?.prompts.length ?? 0,
		clients: report?.clients.length ?? 0
	};
}
