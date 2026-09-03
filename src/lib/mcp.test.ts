import { describe, expect, it } from 'vitest';
import { CLAUDE_SNIPPET, CODEX_SNIPPET, nextDisabledTools, toolIcon } from './mcp';

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
