import { describe, expect, it } from 'vitest';

import { countInStage, groupAssignees, stageIcon } from './boardFilters';
import type { Stage, Task } from '$lib/types';

function task(over: Partial<Task>): Task {
	return {
		id: 'id',
		key: 'ATL-1',
		project_id: null,
		seq: 1,
		title: 'A task',
		description: '',
		stage: 'Backlog',
		kind: 'task',
		priority: 'medium',
		assignee: null,
		labels: [],
		parent_id: null,
		parent_key: null,
		parent_title: null,
		created_by: 'desktop',
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z',
		closed_at: null,
		source_ref: null,
		blocked_by: [],
		open_blockers: 0,
		ready: true,
		blocked_reason: null,
		subtasks_total: 0,
		subtasks_done: 0,
		...over
	};
}

const stage = (name: string, done = false): Stage => ({ name, done });

describe('groupAssignees', () => {
	it('counts each assignee and the tasks nobody has claimed', () => {
		const groups = groupAssignees([
			task({ assignee: 'cli/codex' }),
			task({ assignee: 'cli/codex' }),
			task({ assignee: 'desktop' }),
			task({ assignee: null }),
			task({ assignee: '  ' })
		]);
		expect(groups.named).toEqual([
			['cli/codex', 2],
			['desktop', 1]
		]);
		expect(groups.unassigned).toBe(2);
	});

	it('is empty for an empty board', () => {
		expect(groupAssignees([])).toEqual({ named: [], unassigned: 0 });
	});
});

describe('stageIcon', () => {
	it('gives the frame glyphs to the four columns the design names', () => {
		expect(stageIcon(stage('Backlog'), 0)).toBe('circle');
		expect(stageIcon(stage('In progress'), 1)).toBe('circle-dot');
		expect(stageIcon(stage('Testing'), 2)).toBe('flask-conical');
		expect(stageIcon(stage('Done', true), 3)).toBe('circle-check');
	});

	it('matches Testing by name wherever it sits, and whatever its case', () => {
		expect(stageIcon(stage('TESTING'), 0)).toBe('flask-conical');
	});

	it('falls back on the place in the list for a stage the user named', () => {
		expect(stageIcon(stage('Triage'), 0)).toBe('circle');
		expect(stageIcon(stage('Review'), 2)).toBe('circle-dot');
		expect(stageIcon(stage('Shipped', true), 4)).toBe('circle-check');
	});
});

describe('countInStage', () => {
	it('counts only the tasks standing in that stage', () => {
		const tasks = [task({ stage: 'Backlog' }), task({ stage: 'Done' }), task({ stage: 'Backlog' })];
		expect(countInStage(tasks, 'Backlog')).toBe(2);
		expect(countInStage(tasks, 'Testing')).toBe(0);
	});
});
