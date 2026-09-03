import { describe, expect, it } from 'vitest';
import type { Graph, Workflow } from '$lib/types';

import { lastRunLabel, workflowRows } from './workflows';

function graph(actionCount: number): Graph {
	const nodes: Graph['nodes'] = [
		{ id: 't', kind: 'trigger', position: { x: 0, y: 0 }, data: { kind: 'manual', cron: null, prompt: null } }
	];
	for (let i = 0; i < actionCount; i++) {
		nodes.push({
			id: `a${i}`,
			kind: 'action',
			position: { x: 0, y: 0 },
			data: { name: `a${i}`, instructions: '', agent: 'desktop', practices: [], memories: null }
		});
	}
	nodes.push({ id: 'o', kind: 'output', position: { x: 0, y: 0 }, data: { propose_memories: false, file_tasks: false } });
	return { nodes, edges: [] };
}

function workflow(overrides: Partial<Workflow> = {}): Workflow {
	return {
		id: 'wf-1',
		name: 'morning-digest',
		project_id: 'proj-1',
		description: '',
		trigger: { kind: 'manual', cron: null, prompt: null },
		graph: graph(3),
		enabled: true,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z',
		last_run_at: null,
		last_status: null,
		...overrides
	};
}

describe('workflowRows', () => {
	it('counts only the action-kind nodes', () => {
		const [row] = workflowRows([workflow()]);
		expect(row.actions).toBe(3);
	});

	it('carries the cron only for a schedule trigger', () => {
		const scheduled = workflow({ trigger: { kind: 'schedule', cron: '0 9 * * 1-5', prompt: null } });
		const manual = workflow({ trigger: { kind: 'manual', cron: null, prompt: null } });

		expect(workflowRows([scheduled])[0]).toMatchObject({ triggerKind: 'schedule', cron: '0 9 * * 1-5' });
		expect(workflowRows([manual])[0]).toMatchObject({ triggerKind: 'manual', cron: null });
	});

	it('carries the last run status and timestamp through unchanged', () => {
		const w = workflow({ last_status: 'failed', last_run_at: '2026-09-03T09:00:00Z' });
		expect(workflowRows([w])[0]).toMatchObject({ lastRunStatus: 'failed', lastRunAt: '2026-09-03T09:00:00Z' });
	});

	it('maps an empty workflow list to no rows', () => {
		expect(workflowRows([])).toEqual([]);
	});

	it('keeps workflow order', () => {
		const rows = workflowRows([workflow({ id: 'b', name: 'b' }), workflow({ id: 'a', name: 'a' })]);
		expect(rows.map((r) => r.name)).toEqual(['b', 'a']);
	});
});

describe('lastRunLabel', () => {
	it('reads "never" for a workflow that has not run', () => {
		const [row] = workflowRows([workflow({ last_run_at: null })]);
		expect(lastRunLabel(row)).toBe('never');
	});

	it('reads a relative age for a workflow that has run', () => {
		const now = new Date('2026-09-03T10:00:00Z').getTime();
		const [row] = workflowRows([workflow({ last_run_at: '2026-09-03T08:00:00Z' })]);
		expect(lastRunLabel(row, now)).toBe('2h');
	});
});
