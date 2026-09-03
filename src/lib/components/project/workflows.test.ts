import { describe, expect, it } from 'vitest';
import type { Doc } from '$lib/types';
import { workflowRows } from './workflows';

function doc(name: string): Doc {
	return {
		id: name,
		kind: 'workflow',
		name,
		body: '',
		tags: [],
		project_id: 'proj-1',
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z'
	};
}

describe('workflowRows', () => {
	it('maps a doc to a manual, never-run row with one action', () => {
		expect(workflowRows([doc('morning-digest')])).toEqual([
			{ name: 'morning-digest', trigger: 'manual', actions: 1, lastRun: 'never' }
		]);
	});

	it('maps an empty doc list to no rows', () => {
		expect(workflowRows([])).toEqual([]);
	});

	it('keeps doc order', () => {
		expect(workflowRows([doc('b'), doc('a')]).map((r) => r.name)).toEqual(['b', 'a']);
	});
});
