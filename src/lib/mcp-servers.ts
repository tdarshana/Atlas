// Pure helpers for the MCP servers view, the project MCP tab and the MCP side panel:
// the label an agent draws with, the one-line transport summary, the scope grouping the
// project tab uses, the per-agent counts and the summary line. No Svelte here, so these
// are plain unit tests.

import type {
	McpServerEntry,
	McpServerScope,
	McpServerSource,
	McpServerTransport
} from './types';

/** Each agent under the name it calls itself. */
const SOURCE_LABELS: Record<McpServerSource, string> = {
	claude: 'Claude Code',
	codex: 'Codex',
	cursor: 'Cursor',
	gemini: 'Gemini CLI',
	windsurf: 'Windsurf',
	plugin: 'Plugin',
	atlas: 'Atlas'
};

/** The order the side panel and the Agent select list agents in. */
export const SOURCE_ORDER: McpServerSource[] = [
	'claude',
	'codex',
	'cursor',
	'gemini',
	'windsurf',
	'plugin',
	'atlas'
];

export function sourceLabel(source: McpServerSource): string {
	return SOURCE_LABELS[source] ?? source;
}

/** How much of a command the Transport column shows before it gives up and elides. */
const COMMAND_MAX = 40;

/**
 * One line for the Transport column: `stdio <command>` for a spawned server, the URL's
 * host for an HTTP one. The full transport is in the detail panel, so this only has to
 * be enough to tell two rows apart.
 */
export function transportSummary(transport: McpServerTransport): string {
	if (transport.kind === 'http') {
		try {
			return new URL(transport.url).host;
		} catch {
			return transport.url;
		}
	}
	const command =
		transport.command.length > COMMAND_MAX
			? `${transport.command.slice(0, COMMAND_MAX - 1)}…`
			: transport.command;
	return `stdio ${command}`;
}

export interface ScopeGroup {
	label: string;
	servers: McpServerEntry[];
}

/** What an empty `Project` group says: where `Add server…` would write if it were used. */
export const NO_PROJECT_SERVERS =
	"No project-level servers yet. Add server… writes to this repo's .mcp.json by default.";

/**
 * The project tab's headings, in the order it draws them: the servers configured for
 * this project (Claude Code's project and local entries, Codex's and Cursor's project
 * files), then the plugin servers, then Atlas. A user-scope row keeps its own group
 * rather than being dropped, so a daemon that widens the list is still shown whole.
 *
 * `keepProject` holds the `Project` group even when it is empty, which is what the
 * project tab wants: a repo with no project-level server should still be told where
 * `Add server…` would write, rather than shown nothing.
 */
export function groupByScope(servers: McpServerEntry[], keepProject = false): ScopeGroup[] {
	const groups: ScopeGroup[] = [
		{ label: 'Project', servers: [] },
		{ label: 'Plugins', servers: [] },
		{ label: 'Atlas', servers: [] },
		{ label: 'User', servers: [] }
	];
	const [project, plugins, atlas, user] = groups;

	for (const server of servers) {
		if (server.is_atlas) atlas.servers.push(server);
		else if (server.source === 'plugin' || server.scope === 'plugin') plugins.servers.push(server);
		else if (server.scope === 'project' || server.scope === 'local') project.servers.push(server);
		else user.servers.push(server);
	}

	return groups.filter((g) => g.servers.length > 0 || (keepProject && g === project));
}

export interface AgentCount {
	source: McpServerSource;
	label: string;
	count: number;
}

/** One row per agent that actually contributed a server, for the side panel's AGENTS
 * group and the summary line's agent count. */
export function agentCounts(servers: McpServerEntry[]): AgentCount[] {
	return SOURCE_ORDER.map((source) => ({
		source,
		label: sourceLabel(source),
		count: servers.filter((s) => s.source === source).length
	})).filter((c) => c.count > 0);
}

/** "7 servers across 4 agents, 6 enabled", the line above the table. */
export function enabledSummary(servers: McpServerEntry[]): string {
	const agents = agentCounts(servers).length;
	const enabled = servers.filter((s) => s.enabled).length;
	const serverNoun = servers.length === 1 ? 'server' : 'servers';
	const agentNoun = agents === 1 ? 'agent' : 'agents';
	return `${servers.length} ${serverNoun} across ${agents} ${agentNoun}, ${enabled} enabled`;
}

/**
 * Why a row has no Enabled checkbox, for the static badge's `title`. Enable is offered
 * only where the agent has a switch of its own; everywhere else Remove is the way to
 * stop a server, which is the daemon's own wording.
 */
export function toggleReason(server: McpServerEntry): string {
	if (server.is_atlas) return 'Atlas serves its own MCP server; it is on whenever the daemon is.';
	if (server.source === 'plugin') return 'A plugin server follows the plugin it came from.';
	return `${sourceLabel(server.source)} has no enable switch; remove the server instead.`;
}

// ---- adding a server ---------------------------------------------------------------

export interface AddTarget {
	/** The Select's value; also the key the dialog keeps its choice under. */
	value: string;
	label: string;
	source: McpServerSource;
	scope: McpServerScope;
	/** True where the entry lands in a project's own file, so the target is offered
	 * only when a project is in view. */
	needsProject: boolean;
}

/** Where Atlas is able to write a new entry, in the order the Agent select lists them.
 * Gemini and Windsurf keep only a user-level config, so they have no project target. */
export const ADD_TARGETS: AddTarget[] = [
	{
		value: 'claude:project',
		label: 'Claude Code project (.mcp.json)',
		source: 'claude',
		scope: 'project',
		needsProject: true
	},
	{ value: 'claude:user', label: 'Claude Code user', source: 'claude', scope: 'user', needsProject: false },
	{
		value: 'claude:local',
		label: 'Claude Code local for this project',
		source: 'claude',
		scope: 'local',
		needsProject: true
	},
	{ value: 'codex:user', label: 'Codex user', source: 'codex', scope: 'user', needsProject: false },
	{ value: 'codex:project', label: 'Codex project', source: 'codex', scope: 'project', needsProject: true },
	{ value: 'cursor:user', label: 'Cursor user', source: 'cursor', scope: 'user', needsProject: false },
	{ value: 'cursor:project', label: 'Cursor project', source: 'cursor', scope: 'project', needsProject: true },
	{ value: 'gemini:user', label: 'Gemini CLI user', source: 'gemini', scope: 'user', needsProject: false },
	{ value: 'windsurf:user', label: 'Windsurf user', source: 'windsurf', scope: 'user', needsProject: false }
];

/** The targets offered right now: everything in the global view, the project ones first
 * when a project is in view so the default lands on this project's `.mcp.json`. */
export function addTargets(hasProject: boolean): AddTarget[] {
	if (!hasProject) return ADD_TARGETS.filter((t) => !t.needsProject);
	return [...ADD_TARGETS.filter((t) => t.needsProject), ...ADD_TARGETS.filter((t) => !t.needsProject)];
}

/** The daemon's own rule for a server name, checked here so a typo is caught before the
 * round trip. */
export const NAME_RE = /^[A-Za-z0-9_.-]{1,64}$/;

export function nameError(name: string): string | null {
	const trimmed = name.trim();
	if (trimmed === '') return 'A server needs a name.';
	return NAME_RE.test(trimmed)
		? null
		: 'Letters, digits, dot, dash and underscore, up to 64 characters.';
}

/** `key=value` rows to the object the daemon writes, dropping blank keys. */
export function pairsToObject(pairs: { key: string; value: string }[]): Record<string, string> {
	const out: Record<string, string> = {};
	for (const pair of pairs) {
		const key = pair.key.trim();
		if (key !== '') out[key] = pair.value;
	}
	return out;
}

/** The Arguments textarea, one argument per line, blank lines dropped. */
export function splitArgs(text: string): string[] {
	return text
		.split('\n')
		.map((line) => line.trim())
		.filter((line) => line !== '');
}

// ---- the detail panel's width ------------------------------------------------------

export const DETAIL_KEY = 'atlas.mcp.detail';
export const DETAIL_DEFAULT = 380;
export const DETAIL_MIN = 300;
export const DETAIL_MAX = 640;

export function clampDetail(width: number): number {
	return Number.isFinite(width)
		? Math.min(DETAIL_MAX, Math.max(DETAIL_MIN, Math.round(width)))
		: DETAIL_DEFAULT;
}
