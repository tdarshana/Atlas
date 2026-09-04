import { describe, expect, it } from 'vitest';
import {
	CLAUDE_SNIPPET,
	CODEX_SNIPPET,
	mcpSidepanelCounts,
	nextDisabledTools,
	projectConnectSnippet,
	projectToolState,
	toolIcon
} from './mcp';
import type { McpStatusReport } from './types';

describe('connect snippets', () => {
	it('the Claude snippet is the exact CLI install command', () => {
		expect(CLAUDE_SNIPPET).toBe('claude mcp add --scope user atlas -- atlas mcp');
	});

	it('the Codex snippet is the exact config.toml block', () => {
		expect(CODEX_SNIPPET).toBe(
			'# ~/.codex/config.toml\n[mcp_servers.atlas]\ncommand = "atlas"\nargs = ["mcp"]'
		);
	});
});

describe('nextDisabledTools', () => {
	it('adds the tool when it was just disabled', () => {
		expect(nextDisabledTools(['memory_review'], 'project_connect', false)).toEqual([
			'memory_review',
			'project_connect'
		]);
	});

	it('removes the tool when it was just enabled', () => {
		expect(nextDisabledTools(['memory_review', 'project_connect'], 'memory_review', true)).toEqual(
			['project_connect']
		);
	});

	it('leaves every other name untouched', () => {
		const current = ['memory_review', 'project_connect', 'workflow_run'];
		expect(nextDisabledTools(current, 'workflow_run', true)).toEqual([
			'memory_review',
			'project_connect'
		]);
	});

	it('disabling a tool already disabled does not duplicate it', () => {
		expect(nextDisabledTools(['memory_review'], 'memory_review', false)).toEqual([
			'memory_review'
		]);
	});

	it('enabling a tool that was not disabled is a no-op', () => {
		expect(nextDisabledTools(['memory_review'], 'memory_search', true)).toEqual(['memory_review']);
	});
});

describe('toolIcon', () => {
	it('maps known prefixes', () => {
		expect(toolIcon('memory_remember')).toBe('database');
		expect(toolIcon('project_connect')).toBe('folder');
		expect(toolIcon('task_create')).toBe('columns-3');
		expect(toolIcon('board_stages')).toBe('columns-3');
		expect(toolIcon('practice_list')).toBe('book-open');
		expect(toolIcon('agent_list')).toBe('bot');
		expect(toolIcon('workflow_run')).toBe('git-branch');
		expect(toolIcon('ingest_transcript')).toBe('file');
	});

	it('workflow_status is checked before the bare workflow_ prefix', () => {
		expect(toolIcon('workflow_status')).toBe('history');
	});

	it('falls back to a generic glyph for status and anything unmapped', () => {
		expect(toolIcon('status')).toBe('terminal');
		expect(toolIcon('made_up_tool')).toBe('terminal');
	});
});

describe('projectToolState', () => {
	it('is enabled when the tool is on globally and not overridden here', () => {
		expect(projectToolState({ enabled_globally: true, enabled_here: true })).toBe('enabled');
	});

	it('is disabled_here when the tool is on globally but this project turned it off', () => {
		expect(projectToolState({ enabled_globally: true, enabled_here: false })).toBe('disabled_here');
	});

	it('is disabled_globally when the tool is off globally, whatever this project says', () => {
		expect(projectToolState({ enabled_globally: false, enabled_here: false })).toBe('disabled_globally');
	});
});

describe('projectConnectSnippet', () => {
	it('carries the project-scoped install command and the project_root hint', () => {
		expect(projectConnectSnippet('/Users/me/repo')).toBe(
			'claude mcp add --scope project atlas -- atlas mcp\n# project_root: /Users/me/repo'
		);
	});
});

describe('mcpSidepanelCounts', () => {
	const report: McpStatusReport = {
		transports: { stdio: { command: 'atlas mcp' }, http: { url: '', protocol_version: '' } },
		counts: { tools: 2, resources: 1, prompts: 1, clients: 1 },
		tools: [
			{ name: 'a', description: '', args: '', scope: 'read', enabled: true },
			{ name: 'b', description: '', args: '', scope: 'read', enabled: false }
		],
		resources: [{ uri: 'atlas://memories/recent', name: 'recent' }],
		prompts: [{ name: 'atlas.bootstrap' }],
		clients: [
			{
				id: '1',
				transport: 'stdio',
				client_name: 'claude',
				client_version: null,
				first_seen: '',
				last_seen: '',
				tool_calls: 0,
				last_project_id: null
			}
		]
	};

	it('counts total and disabled tools, and the resource/prompt/client lists', () => {
		expect(mcpSidepanelCounts(report)).toEqual({
			tools: 2,
			toolsDisabled: 1,
			resources: 1,
			prompts: 1,
			clients: 1
		});
	});

	it('reads every count as 0 before the first load', () => {
		expect(mcpSidepanelCounts(null)).toEqual({
			tools: 0,
			toolsDisabled: 0,
			resources: 0,
			prompts: 0,
			clients: 0
		});
	});
});
