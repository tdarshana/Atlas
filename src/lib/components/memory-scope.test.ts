import { describe, expect, it } from 'vitest';
import { ALL_SCOPE, scopeOptions, scopeSelection, scopeValue } from './memory-scope';
import type { Project } from '$lib/types';

function project(id: string, name: string): Project {
	return {
		id,
		name,
		root_path: `/home/me/${name}`,
		git_remote: null,
		profile: null,
		created_at: '2026-09-01T00:00:00Z',
		last_seen_at: '2026-09-01T00:00:00Z',
		board_key: null,
		board_stages: null,
		agent_access: { memory_writers: null, task_movers: null, require_review: false },
		extraction: null,
		mcp_disabled_tools: [],
		skills_disabled: []
	};
}

const atlas = project('p-atlas', 'atlas');
const habit = project('p-habit', 'Habitmaker');

describe('scopeOptions', () => {
	it('puts All first and then every project by name', () => {
		expect(scopeOptions([atlas, habit])).toEqual([
			{ value: 'all', label: 'All' },
			{ value: 'p-atlas', label: 'atlas' },
			{ value: 'p-habit', label: 'Habitmaker' }
		]);
	});

	it('is just All when no project is connected', () => {
		expect(scopeOptions([])).toEqual([{ value: 'all', label: 'All' }]);
	});
});

describe('scopeSelection', () => {
	it('maps All to no scope filter and no project', () => {
		expect(scopeSelection(ALL_SCOPE)).toEqual({ scope: 'all', projectId: '' });
	});

	it('maps a project row to a project-scoped request for that id', () => {
		expect(scopeSelection('p-atlas')).toEqual({ scope: 'project', projectId: 'p-atlas' });
	});
});

describe('scopeValue', () => {
	it('round trips a project selection', () => {
		const chosen = scopeSelection('p-habit');
		expect(scopeValue(chosen.scope, chosen.projectId)).toBe('p-habit');
	});

	it('reads All back for the all filter', () => {
		expect(scopeValue('all', '')).toBe(ALL_SCOPE);
	});

	it('reads All back for a project filter with no project chosen', () => {
		expect(scopeValue('project', '')).toBe(ALL_SCOPE);
	});
});
