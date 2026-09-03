// Pure helpers for the Settings screen's MCP server card (frame 10): the two connect
// snippets, the disabled-tools diff a checkbox toggle sends, and the icon a tool's
// name maps to. No Svelte here, so these are plain unit tests.

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
