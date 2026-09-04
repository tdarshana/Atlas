// The pure helpers behind the MCP servers table: the agent label a row badges with, the
// one-line transport summary the Transport column shows, the scope grouping the project
// tab draws, the per-agent counts the side panel lists, and the summary line above the
// table. No Svelte here, so these are plain unit tests.

import { describe, expect, it } from 'vitest';
import {
	agentCounts,
	enabledSummary,
	groupByScope,
	sourceLabel,
	toggleReason,
	transportSummary
} from './mcp-servers';
import type { McpServerEntry, McpServerSource, McpServerTransport } from './types';

const stdio: McpServerTransport = {
	kind: 'stdio',
	command: 'npx',
	args: ['-y', '@modelcontextprotocol/server-filesystem'],
	env_keys: []
};

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
		transport: stdio,
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

const atlas = entry('atlas', 'atlas', {
	id: 'atlas',
	file: null,
	can_toggle: false,
	can_remove: false,
	is_atlas: true,
	transport: { kind: 'stdio', command: 'atlas', args: ['mcp'], env_keys: [] }
});

describe('sourceLabel', () => {
	it('names each agent the way that agent names itself', () => {
		expect(sourceLabel('claude')).toBe('Claude Code');
		expect(sourceLabel('codex')).toBe('Codex');
		expect(sourceLabel('cursor')).toBe('Cursor');
		expect(sourceLabel('gemini')).toBe('Gemini CLI');
		expect(sourceLabel('windsurf')).toBe('Windsurf');
		expect(sourceLabel('plugin')).toBe('Plugin');
		expect(sourceLabel('atlas')).toBe('Atlas');
	});
});

describe('transportSummary', () => {
	it('shows the command for stdio', () => {
		expect(transportSummary({ kind: 'stdio', command: 'atlas', args: ['mcp'], env_keys: [] })).toBe(
			'stdio atlas'
		);
	});

	it('truncates a long command rather than pushing the column wide', () => {
		const long = '/opt/homebrew/Cellar/node/22.0.0/bin/node-with-a-very-long-path-indeed';
		const text = transportSummary({ kind: 'stdio', command: long, args: [], env_keys: [] });
		expect(text.length).toBeLessThanOrEqual('stdio '.length + 40);
		expect(text.endsWith('…')).toBe(true);
	});

	it('shows the host for HTTP, not the whole URL', () => {
		expect(
			transportSummary({
				kind: 'http',
				url: 'https://mcp.example.com/v1/sse?token=abc',
				header_keys: []
			})
		).toBe('mcp.example.com');
	});

	it('falls back to the raw text when the URL will not parse', () => {
		expect(transportSummary({ kind: 'http', url: 'not a url', header_keys: [] })).toBe('not a url');
	});
});

describe('groupByScope', () => {
	it('puts the project scopes first, then plugins, then Atlas', () => {
		const groups = groupByScope([
			atlas,
			entry('docs', 'plugin', { scope: 'plugin', plugin: 'anthropics/docs' }),
			entry('repo', 'claude', { scope: 'project' }),
			entry('local', 'claude', { scope: 'local' })
		]);

		expect(groups.map((g) => g.label)).toEqual(['Project', 'Plugins', 'Atlas']);
		expect(groups[0].servers.map((s) => s.name)).toEqual(['repo', 'local']);
		expect(groups[1].servers.map((s) => s.name)).toEqual(['docs']);
		expect(groups[2].servers.map((s) => s.name)).toEqual(['atlas']);
	});

	it('leaves out a group with nothing in it', () => {
		expect(groupByScope([entry('repo', 'claude', { scope: 'project' })]).map((g) => g.label)).toEqual(
			['Project']
		);
	});

	it('keeps an empty Project group when the project tab asks for it', () => {
		const groups = groupByScope([atlas], true);
		expect(groups.map((g) => g.label)).toEqual(['Project', 'Atlas']);
		expect(groups[0].servers).toEqual([]);
	});

	it('still leaves out the other empty groups when Project is kept', () => {
		expect(groupByScope([], true).map((g) => g.label)).toEqual(['Project']);
	});

	it('keeps a user-scope row rather than dropping it', () => {
		const groups = groupByScope([entry('global', 'codex')]);
		expect(groups.map((g) => g.label)).toEqual(['User']);
	});
});

describe('agentCounts', () => {
	it('counts the servers each agent contributed, in a fixed order', () => {
		const counts = agentCounts([
			entry('a', 'claude'),
			entry('b', 'claude'),
			entry('c', 'cursor'),
			atlas
		]);

		expect(counts).toEqual([
			{ source: 'claude', label: 'Claude Code', count: 2 },
			{ source: 'cursor', label: 'Cursor', count: 1 },
			{ source: 'atlas', label: 'Atlas', count: 1 }
		]);
	});
});

describe('enabledSummary', () => {
	it('counts servers, the agents they came from, and how many are on', () => {
		expect(
			enabledSummary([entry('a', 'claude'), entry('b', 'cursor', { enabled: false }), atlas])
		).toBe('3 servers across 3 agents, 2 enabled');
	});

	it('says it in the singular for one server from one agent', () => {
		expect(enabledSummary([entry('a', 'claude')])).toBe('1 server across 1 agent, 1 enabled');
	});

	it('says so when nothing was found', () => {
		expect(enabledSummary([])).toBe('0 servers across 0 agents, 0 enabled');
	});
});

describe('toggleReason', () => {
	it('explains why Atlas has no switch', () => {
		expect(toggleReason(atlas)).toContain('Atlas');
	});

	it('explains that a plugin server follows its plugin', () => {
		expect(
			toggleReason(entry('docs', 'plugin', { scope: 'plugin', can_toggle: false }))
		).toContain('plugin');
	});

	it('points at Remove for an agent with no switch of its own', () => {
		expect(toggleReason(entry('a', 'codex', { can_toggle: false }))).toContain(
			'remove the server instead'
		);
	});
});
